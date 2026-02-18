// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::anyhow;
use serde::Deserialize;

const INTERPRETER_PY: &str = include_str!("interpreter.py");

#[derive(Debug, Deserialize)]
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
    pub micro: u8,
    pub releaselevel: String,
    pub serial: u8,
}

#[derive(Debug, Deserialize)]
pub struct MarkerEnv {
    pub os_name: String,
    pub sys_platform: String,
    pub platform_machine: String,
    pub platform_python_implementation: String,
    pub platform_release: String,
    pub platform_system: String,
    pub platform_version: String,
    pub python_version: String,
    pub python_full_version: String,
    pub implementation_name: String,
    pub implementation_version: String,
}

#[derive(Debug, Deserialize)]
pub struct Interpreter {
    pub path: PathBuf,
    pub realpath: PathBuf,
    pub prefix: PathBuf,
    pub base_prefix: Option<PathBuf>,
    pub version: PythonVersion,
    pub marker_env: MarkerEnv,
    pub macos_framework_build: bool,
    pub supported_tags: Vec<String>,
    pub has_ensurepip: bool,
}

impl Interpreter {
    pub fn load(python_exe: impl AsRef<Path>) -> anyhow::Result<Interpreter> {
        let mut command = Command::new(python_exe.as_ref());
        command.arg("-sE").arg("-c").arg(INTERPRETER_PY);
        #[cfg(target_os = "linux")]
        {
            use crate::linux::LinuxInfo;
            let linux_info = LinuxInfo::parse(python_exe.as_ref())?;
            let json = serde_json::to_string(&linux_info)?;
            command.arg("--linux-info").arg(json);
        }
        let result = command.stdout(Stdio::piped()).spawn()?.wait_with_output()?;
        serde_json::from_slice(result.stdout.as_slice()).map_err(|err| {
            anyhow!(
                "Failed to identify Python interpreter {exe}: {err}",
                exe = python_exe.as_ref().display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use textwrap::dedent;

    use crate::Interpreter;

    #[test]
    fn test_tags_same_as_packaging() {
        let venv_dir = tempfile::tempdir().unwrap();
        let python_exe = {
            let python_exe_basename = {
                let python_exe_bytes = Command::new("uv")
                    .args(["python", "find"])
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap()
                    .wait_with_output()
                    .unwrap()
                    .stdout;
                let python_exe = PathBuf::from(String::from_utf8(python_exe_bytes).unwrap().trim());
                Command::new(&python_exe)
                    .args(["-m", "venv"])
                    .arg(venv_dir.path())
                    .spawn()
                    .unwrap()
                    .wait()
                    .unwrap();
                OsString::from(python_exe.file_name().unwrap())
            };

            let python_exe = if build_target::target_os() == build_target::Os::Windows {
                venv_dir.path().join("Scripts")
            } else {
                venv_dir.path().join("bin")
            }
            .join(python_exe_basename);
            Command::new(&python_exe)
                .args(["-m", "pip", "install", "packaging"])
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
            python_exe
        };

        let tags_output = Command::new(&python_exe)
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
        let interpreter = Interpreter::load(python_exe).unwrap();
        assert_eq!(expected_tags, interpreter.supported_tags);
    }
}
