// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::{anyhow, bail};
use cache::{CacheDir, HashOptions, atomic_file, hash_file};
use fs_err as fs;
use log::debug;
use logging_timer::time;
use pep508_rs::MarkerEnvironment;
use resources::{InterpreterIdentificationScript, Resources};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
    pub micro: u8,
    pub releaselevel: String,
    pub serial: u8,
}

impl Display for PythonVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{major}.{minor}.{micro}",
            major = self.major,
            minor = self.minor,
            micro = self.micro
        ))?;

        // N.B.: Using this for possible strings reference:
        // https://peps.python.org/pep-0739/#implementation-version-releaselevel

        if let Some(level_abbrev) = match self.releaselevel.as_str() {
            "alpha" => Some("a"),
            "beta" => Some("b"),
            "candidate" => Some("rc"),
            _ => None,
        } {
            f.write_fmt(format_args!("{level_abbrev}{serial}", serial = self.serial))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PyPyVersion(u8, u8, u8);

impl Display for PyPyVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{major}.{minor}.{patch}",
            major = self.0,
            minor = self.1,
            patch = self.2
        ))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Interpreter {
    pub path: PathBuf,
    pub realpath: PathBuf,
    pub prefix: PathBuf,
    pub base_prefix: Option<PathBuf>,
    pub version: PythonVersion,
    pub pypy_version: Option<PyPyVersion>,
    pub marker_env: MarkerEnvironment,
    pub macos_framework_build: bool,
    pub supported_tags: Vec<String>,
    pub has_ensurepip: bool,
    pub free_threaded: Option<bool>,
}

#[cfg(target_os = "linux")]
static LINUX_INFO: Mutex<Option<String>> = Mutex::new(None);

impl Interpreter {
    fn identify(
        python_exe: impl AsRef<Path>,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Vec<u8>> {
        let mut script = tempfile::Builder::new()
            .prefix("virtualenv.")
            .suffix(".py")
            .tempfile()?;
        script.write_all(identification_script.contents().as_bytes())?;
        let mut command = Command::new(python_exe.as_ref());
        command.arg("-sE").arg(script.path());
        #[cfg(target_os = "linux")]
        {
            let mut linux_info = LINUX_INFO
                .lock()
                .map_err(|err| anyhow!("Failed to obtain lock on Linux platform info: {err}"))?;
            let json = if let Some(json) = linux_info.as_ref() {
                debug!("Using cached Linux info.");
                json
            } else {
                let info = crate::linux::LinuxInfo::parse(python_exe.as_ref())?;
                let json = serde_json::to_string(&info)?;
                debug!(
                    "Caching Linux info derived from {path}.",
                    path = python_exe.as_ref().display()
                );
                linux_info.insert(json)
            };
            command.arg("--linux-info").arg(json);
        }
        let result = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .wait_with_output()?;
        if !result.status.success() {
            bail!(
                "Failed to identify Python interpreter at {path}.\n\
                Exit status {status} with STDERR:\n{stderr}",
                path = python_exe.as_ref().display(),
                status = result.status,
                stderr = String::from_utf8_lossy(result.stderr.as_slice())
            )
        }
        Ok(result.stdout)
    }

    pub fn load_uncached(
        python_exe: impl AsRef<Path>,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Self> {
        let json_bytes = Self::identify(python_exe.as_ref(), identification_script)?;
        serde_json::from_slice(&json_bytes).map_err(|err| {
            anyhow!(
                "Failed to identify Python interpreter {exe}: {err}",
                exe = python_exe.as_ref().display()
            )
        })
    }

    #[cfg(unix)]
    pub fn most_specific_exe_name(&self) -> String {
        let name = if self.pypy_version.is_some() {
            "pypy"
        } else {
            "python"
        };
        format!(
            "{name}{major}.{minor}",
            major = self.version.major,
            minor = self.version.minor
        )
    }

    #[cfg(unix)]
    fn candidate_rel_paths<'a>(
        version: &PythonVersion,
        pypy_version: Option<&PyPyVersion>,
    ) -> Vec<Cow<'a, Path>> {
        let mut candidates: Vec<Cow<'_, Path>> =
            Vec::with_capacity(if pypy_version.is_some() { 6 } else { 3 });
        if pypy_version.is_some() {
            candidates.push(Cow::Owned(PathBuf::from(format!(
                "bin/pypy{major}.{minor}",
                major = version.major,
                minor = version.minor
            ))));
            candidates.push(Cow::Owned(PathBuf::from(format!(
                "bin/pypy{major}",
                major = version.major
            ))));
            candidates.push(Cow::Borrowed(Path::new("bin/pypy")));
        }
        candidates.push(Cow::Owned(PathBuf::from(format!(
            "bin/python{major}.{minor}",
            major = version.major,
            minor = version.minor
        ))));
        candidates.push(Cow::Owned(PathBuf::from(format!(
            "bin/python{major}",
            major = version.major
        ))));
        candidates.push(Cow::Borrowed(Path::new("bin/python")));
        candidates
    }

    #[cfg(windows)]
    fn candidate_rel_paths<'a>(
        version: &PythonVersion,
        pypy_version: Option<&PyPyVersion>,
    ) -> Vec<Cow<'a, Path>> {
        if let Some(pypy_version) = pypy_version {
            vec![
                Cow::Borrowed(Path::new("pypy.exe")),
                Cow::Borrowed(Path::new("python.exe")),
                Cow::Owned(PathBuf::from(format!(
                    "Scripts\\pypy{major}.{minor}.exe",
                    major = pypy_version.0,
                    minor = pypy_version.1
                ))),
                Cow::Owned(PathBuf::from(format!(
                    "Scripts\\pypy{major}.exe",
                    major = pypy_version.0
                ))),
                Cow::Borrowed(Path::new("Scripts\\pypy.exe")),
                Cow::Owned(PathBuf::from(format!(
                    "Scripts\\python{major}.{minor}.exe",
                    major = version.major,
                    minor = version.minor
                ))),
                Cow::Owned(PathBuf::from(format!(
                    "Scripts\\python{major}.exe",
                    major = version.major
                ))),
                Cow::Borrowed(Path::new("Scripts\\python.exe")),
            ]
        } else {
            vec![
                Cow::Borrowed(Path::new("python.exe")),
                Cow::Borrowed(Path::new("Scripts\\python.exe")),
            ]
        }
    }

    fn at_prefix<'a>(
        prefix: impl AsRef<Path>,
        version: PythonVersion,
        pypy_version: Option<PyPyVersion>,
        resources: &mut impl Resources<'a>,
        re_cache_version_mismatch: bool,
    ) -> anyhow::Result<Self> {
        let check_pypy_version = |interpreter: &Interpreter| match (
            pypy_version.as_ref(),
            interpreter.pypy_version.as_ref(),
        ) {
            (Some(expected_pypy_version), Some(actual_pypy_version))
                if expected_pypy_version == actual_pypy_version =>
            {
                true
            }
            (None, None) => true,
            _ => false,
        };
        let identification_script = InterpreterIdentificationScript::read(resources)?;
        let candidate_rel_paths = Self::candidate_rel_paths(&version, pypy_version.as_ref());
        let mut re_cache_candidates: Vec<Self> = Vec::with_capacity(candidate_rel_paths.len());
        for rel_path in candidate_rel_paths {
            let candidate_path = prefix.as_ref().join(rel_path);
            if let Ok(interpreter) = Self::load(candidate_path, &identification_script) {
                if interpreter.version != version {
                    if re_cache_version_mismatch
                        && (interpreter.version.major, interpreter.version.minor)
                            == (version.major, version.minor)
                    {
                        re_cache_candidates.push(interpreter)
                    }
                    continue;
                }
                if check_pypy_version(&interpreter) {
                    return Ok(interpreter);
                }
            }
        }
        for interpreter in re_cache_candidates {
            let interpreter = interpreter.reload(&identification_script)?;
            if interpreter.version == version && check_pypy_version(&interpreter) {
                return Ok(interpreter);
            }
        }
        if let Some(pypy_version) = pypy_version {
            bail!(
                "Failed to find a Python interpreter matching version {version} \
                (PyPy {pypy_version})"
            )
        } else {
            bail!("Failed to find a Python interpreter matching version {version}")
        }
    }

    const INTERPRETER_HASH_CONFIG: HashOptions =
        HashOptions::new().path(true).mtime(true).size(true);

    fn interpreter_info(python_exe: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let hash = hash_file(python_exe.as_ref(), &Self::INTERPRETER_HASH_CONFIG)?;
        Ok(CacheDir::Interpreter.path()?.join(hash.base64_digest()))
    }

    #[time("debug", "Interpreter.{}")]
    pub fn load(
        python_exe: impl AsRef<Path>,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Self> {
        let interpreter_info = Self::interpreter_info(python_exe.as_ref())?;
        Self::load_internal(interpreter_info, python_exe, identification_script)
    }

    fn load_internal(
        interpreter_info: impl AsRef<Path>,
        python_exe: impl AsRef<Path>,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Self> {
        let file = atomic_file(interpreter_info.as_ref(), |file| {
            let json_bytes = Self::identify(python_exe.as_ref(), identification_script)?;
            BufWriter::new(file).write_all(&json_bytes)?;
            Ok(())
        })?;
        serde_json::from_reader(BufReader::new(file)).map_err(|err| {
            anyhow!(
                "Failed to identify Python interpreter {exe}: {err}",
                exe = python_exe.as_ref().display()
            )
        })
    }

    fn reload(
        self,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Self> {
        let interpreter_info = Self::interpreter_info(self.path.as_path())?;
        fs::remove_file(&interpreter_info)?;
        Self::load_internal(&interpreter_info, self.path, identification_script)
    }

    #[time("debug", "Interpreter.{}")]
    pub fn store(&self) -> anyhow::Result<()> {
        let hash = hash_file(&self.path, &Self::INTERPRETER_HASH_CONFIG)?;
        let interpreter_info = CacheDir::Interpreter.path()?.join(hash.base64_digest());
        atomic_file(&interpreter_info, |file| {
            serde_json::to_writer(BufWriter::new(file), self)?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn hermetic_args(&self) -> &'static str {
        if self.version.major == 3 && self.version.minor >= 4 {
            "-I"
        } else {
            "-sE"
        }
    }

    #[time("debug", "Interpreter.{}")]
    pub fn resolve_base_interpreter<'a>(
        self,
        resources: &mut impl Resources<'a>,
    ) -> anyhow::Result<Interpreter> {
        if let Some(base_prefix) = self.base_prefix.as_ref()
            && base_prefix != &self.prefix
        {
            let resolved = Self::at_prefix(
                base_prefix,
                self.version,
                self.pypy_version,
                resources,
                true,
            )?;
            return resolved.resolve_base_interpreter(resources);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {

    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use anyhow::Context;
    use resources::{InterpreterIdentificationScript, Resources};
    use rstest::rstest;
    use testing::{
        embedded_resources,
        interpreter_identification_script,
        python_exe,
        venv_python_exe,
    };
    use textwrap::dedent;

    use crate::Interpreter;

    #[rstest]
    fn test_tags_same_as_packaging(
        venv_python_exe: PathBuf,
        interpreter_identification_script: InterpreterIdentificationScript,
    ) {
        assert!(
            Command::new(&venv_python_exe)
                .args(["-m", "pip", "install", "packaging"])
                .spawn()
                .unwrap()
                .wait()
                .unwrap()
                .success()
        );
        let output = Command::new(&venv_python_exe)
            .arg("-c")
            .arg(dedent(
                "
                import json
                import sys

                from packaging import tags

                json.dump(list(map(str, tags.sys_tags())), sys.stdout)
                ",
            ))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap()
            .wait_with_output()
            .unwrap();
        assert!(output.status.success());
        let expected_tags: Vec<String> =
            serde_json::from_str(String::from_utf8(output.stdout).unwrap().as_str()).unwrap();

        let interpreter =
            Interpreter::load_uncached(&venv_python_exe, &interpreter_identification_script)
                .with_context(|| {
                    format!(
                        "Failed to load interpreter info for {python}",
                        python = venv_python_exe.display()
                    )
                })
                .unwrap();
        assert_eq!(expected_tags, interpreter.supported_tags);
    }

    #[rstest]
    fn test_resolve_base_interpreter(
        python_exe: &Path,
        venv_python_exe: PathBuf,
        mut embedded_resources: impl Resources<'static>,
    ) {
        let identification_script =
            InterpreterIdentificationScript::read(&mut embedded_resources).unwrap();
        let venv_interpreter = Interpreter::load(&venv_python_exe, &identification_script)
            .with_context(|| {
                format!(
                    "Failed to load interpreter info for {python}",
                    python = venv_python_exe.display()
                )
            })
            .unwrap();
        assert_eq!(
            python_exe,
            venv_interpreter
                .resolve_base_interpreter(&mut embedded_resources)
                .unwrap()
                .path
        )
    }
}
