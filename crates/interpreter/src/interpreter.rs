// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::{anyhow, bail};
use cache::{CacheDir, HashOptions, atomic_file, hash_file};
use log::debug;
use logging_timer::time;
use pep508_rs::MarkerEnvironment;
use python::InterpreterIdentificationScript;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
    pub micro: u8,
    pub releaselevel: String,
    pub serial: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PyPyVersion(u8, u8, u8);

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

impl Interpreter {
    pub fn hermetic_args(&self) -> &'static str {
        if self.version.major == 3 && self.version.minor >= 4 {
            "-I"
        } else {
            "-sE"
        }
    }
}

#[cfg(target_os = "linux")]
static LINUX_INFO: Mutex<Option<String>> = Mutex::new(None);

impl Interpreter {
    fn identify(
        python_exe: impl AsRef<Path>,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Vec<u8>> {
        let mut command = Command::new(python_exe.as_ref());
        command
            .arg("-sE")
            .arg("-c")
            .arg(identification_script.contents());
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

    const INTERPRETER_HASH_CONFIG: HashOptions =
        HashOptions::new().path(true).mtime(true).size(true);

    #[time("debug", "Interpreter.{}")]
    pub fn load<'a>(
        python_exe: impl AsRef<Path>,
        identification_script: &InterpreterIdentificationScript,
    ) -> anyhow::Result<Self> {
        let hash = hash_file(python_exe.as_ref(), &Self::INTERPRETER_HASH_CONFIG)?;
        let interpreter_info = CacheDir::Interpreter.path()?.join(hash.base64_digest());
        let file = atomic_file(&interpreter_info, |file| {
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
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use python::InterpreterIdentificationScript;
    use rstest::rstest;
    use testing::{interpreter_identification_script, venv_python_exe};
    use textwrap::dedent;

    use crate::Interpreter;

    #[rstest]
    fn test_tags_same_as_packaging(
        venv_python_exe: PathBuf,
        interpreter_identification_script: InterpreterIdentificationScript,
    ) {
        Command::new(&venv_python_exe)
            .args(["-m", "pip", "install", "packaging"])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        let tags_output = Command::new(&venv_python_exe)
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
            .unwrap()
            .stdout;
        let expected_tags: Vec<String> =
            serde_json::from_str(String::from_utf8(tags_output).unwrap().as_str()).unwrap();

        let interpreter =
            Interpreter::load_uncached(venv_python_exe, &interpreter_identification_script)
                .unwrap();
        assert_eq!(expected_tags, interpreter.supported_tags);
    }
}
