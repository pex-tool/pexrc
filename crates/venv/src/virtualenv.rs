// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::env;
use std::env::consts::EXE_SUFFIX;
use std::io::Write;
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail};
use const_format::concatcp;
use fs_err as fs;
use interpreter::Interpreter;
use logging_timer::time;
use platform::symlink_or_link_or_copy;
use scripts::{IdentifyInterpreter, Scripts, VendoredVirtualenv};
use target_lexicon::{HOST, OperatingSystem};

const SCRIPTS_DIR: &str = env!("SCRIPTS_DIR");
const PYTHON_EXE: &str = concatcp!("python", EXE_SUFFIX);
const VENV_PYTHON_RELPATH: &str = concatcp!(SCRIPTS_DIR, MAIN_SEPARATOR_STR, PYTHON_EXE);

pub trait Linker {
    fn link(&self, interpreter: &Interpreter, path: &Path) -> anyhow::Result<()>;
}

pub struct FileSystemLinker();

impl Linker for FileSystemLinker {
    fn link(&self, interpreter: &Interpreter, path: &Path) -> anyhow::Result<()> {
        symlink_or_link_or_copy(&interpreter.realpath, path, false)?;
        Ok(())
    }
}

pub struct Virtualenv<'a> {
    pub interpreter: Interpreter,
    pub bin_dir_relpath: &'a str,
    site_packages_relpath: Cow<'a, Path>,
}

impl<'a> Virtualenv<'a> {
    #[time("debug", "Virtualenv.{}")]
    pub fn enclosing(interpreter: Interpreter) -> anyhow::Result<Self> {
        let site_packages_relpath = site_packages_relpath(&interpreter);
        Ok(Self {
            interpreter,
            bin_dir_relpath: SCRIPTS_DIR,
            site_packages_relpath,
        })
    }

    #[time("debug", "Virtualenv.{}")]
    pub fn load(path: Cow<'a, Path>, scripts: &mut Scripts) -> anyhow::Result<Self> {
        let identification_script = IdentifyInterpreter::read(scripts)?;
        let interpreter = Interpreter::load(
            path.as_ref().join(VENV_PYTHON_RELPATH),
            &identification_script,
        )?;
        Self::enclosing(interpreter)
    }

    pub fn host_interpreter(venv_dir: &Path, interpreter: &Interpreter) -> Interpreter {
        let mut venv_interpreter = interpreter.clone();
        venv_interpreter.base_prefix = venv_interpreter
            .base_prefix
            .or(Some(venv_interpreter.prefix));
        venv_interpreter.prefix = venv_dir.to_path_buf();
        venv_interpreter.path = venv_dir.join(VENV_PYTHON_RELPATH);
        if HOST.operating_system == OperatingSystem::Windows {
            venv_interpreter.realpath = venv_interpreter.path.clone();
        }
        venv_interpreter
    }

    #[time("debug", "Virtualenv.{}")]
    pub fn create(
        interpreter: Interpreter,
        path: Cow<'a, Path>,
        linker: impl Linker,
        scripts: &mut Scripts,
        include_system_site_packages: bool,
    ) -> anyhow::Result<Self> {
        let venv_interpreter = Self::host_interpreter(path.as_ref(), &interpreter);

        let site_packages_relpath =
            if interpreter.version.major == 3 && interpreter.version.minor >= 3 {
                create_pep_405_venv(
                    interpreter,
                    path.as_ref(),
                    linker,
                    include_system_site_packages,
                    scripts,
                )?
            } else {
                let virtualenv_script = VendoredVirtualenv::read(scripts)?;
                create_virtualenv_venv(
                    &interpreter,
                    path.as_ref(),
                    linker,
                    virtualenv_script,
                    include_system_site_packages,
                )?
            };

        Ok(Self {
            interpreter: venv_interpreter,
            bin_dir_relpath: SCRIPTS_DIR,
            site_packages_relpath,
        })
    }

    pub fn prefix(&self) -> &Path {
        &self.interpreter.prefix
    }

    pub fn site_packages_path(&self) -> PathBuf {
        self.interpreter.prefix.join(&self.site_packages_relpath)
    }
}

fn create_pep_405_venv<'a>(
    interpreter: Interpreter,
    path: &Path,
    linker: impl Linker,
    include_system_site_packages: bool,
    scripts: &mut Scripts,
) -> anyhow::Result<Cow<'a, Path>> {
    // See: https://peps.python.org/pep-0405/
    let base_interpreter = interpreter.resolve_base_interpreter(scripts)?;
    let home = base_interpreter.realpath.parent().ok_or_else(|| {
        anyhow!(
            "Failed to calculate the home dir of venv base python {path}",
            path = base_interpreter.realpath.display()
        )
    })?;
    fs::write(
        path.join("pyvenv.cfg"),
        format!(
            "\
            home = {home}\n\
            include-system-site-packages = {include_system_site_packages}\n\
            ",
            home = home.display()
        ),
    )?;
    let scripts_dir = path.join(SCRIPTS_DIR);
    fs::create_dir_all(&scripts_dir)?;
    linker.link(&base_interpreter, &scripts_dir.join(PYTHON_EXE))?;
    let site_packages_relpath = site_packages_relpath(&base_interpreter);
    fs::create_dir_all(path.join(site_packages_relpath.as_ref()))?;
    Ok(site_packages_relpath)
}

fn create_virtualenv_venv<'a>(
    interpreter: &Interpreter,
    path: &Path,
    _linker: impl Linker,
    virtualenv_script: VendoredVirtualenv<'a>,
    include_system_site_packages: bool,
) -> anyhow::Result<Cow<'a, Path>> {
    let mut script = tempfile::Builder::new()
        .prefix("virtualenv.")
        .suffix(".py")
        .tempfile()?;
    script.write_all(virtualenv_script.contents().as_bytes())?;
    let mut command = Command::new(&interpreter.path);
    command
        .arg(interpreter.hermetic_args())
        .arg(script.path())
        .args(["--no-pip", "--no-setuptools", "--no-wheel"]);
    if include_system_site_packages {
        command.arg("--system-site-packages");
    }
    let child = command
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "Failed to create a venv at {workdir} using {python_implementation} {python_version} \
            interpreter {python_exe}:\n{stderr}",
            workdir = path.display(),
            python_implementation = interpreter.marker_env.platform_python_implementation(),
            python_version = interpreter.marker_env.python_full_version(),
            python_exe = interpreter.path.display(),
            stderr = String::from_utf8_lossy(&output.stderr)
        )
    }
    // TODO: XXX: Replace the non-symlink pythons with a python-proxy (+ hardlinks for extras?).
    //  It's not yet clear if this can be done under virtualenv + with -E.
    Ok(site_packages_relpath(interpreter))
}

fn site_packages_relpath<'a>(interpreter: &Interpreter) -> Cow<'a, Path> {
    if HOST.operating_system == OperatingSystem::Windows {
        // TODO: XXX: Confirm venv layouts for PyPy under Windows.
        Cow::Borrowed(Path::new("Lib\\site-packages"))
    } else if interpreter.marker_env.platform_python_implementation() == "PyPy"
        && (interpreter.version.major, interpreter.version.minor) < (3, 8)
    {
        Cow::Borrowed(Path::new("site-packages"))
    } else {
        Cow::Owned(
            PathBuf::from("lib")
                .join(format!(
                    "{implementation}{major}.{minor}",
                    implementation =
                        if interpreter.marker_env.platform_python_implementation() == "PyPy" {
                            "pypy"
                        } else {
                            "python"
                        },
                    major = interpreter.version.major,
                    minor = interpreter.version.minor
                ))
                .join("site-packages"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;

    use interpreter::Interpreter;
    use rstest::rstest;
    use scripts::{IdentifyInterpreter, Scripts};
    use testing::{embedded_scripts, python_exe, tmp_dir};

    use crate::virtualenv::{FileSystemLinker, Path, Virtualenv};

    #[rstest]
    fn test_create(python_exe: &Path, tmp_dir: PathBuf, mut embedded_scripts: Scripts) {
        let identification_script = IdentifyInterpreter::read(&mut embedded_scripts).unwrap();
        let interpreter = Interpreter::load(python_exe, &identification_script).unwrap();
        let expected_prefix = interpreter
            .base_prefix
            .as_ref()
            .unwrap_or(&interpreter.prefix)
            .clone();
        let venv = Virtualenv::create(
            interpreter,
            Cow::Owned(tmp_dir),
            FileSystemLinker(),
            &mut embedded_scripts,
            false,
        )
        .unwrap();
        assert_eq!(expected_prefix, venv.interpreter.base_prefix.unwrap())
    }
}
