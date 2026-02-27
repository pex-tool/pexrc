// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::anyhow;
use logging_timer::time;
use pep508_rs::MarkerEnvironment;
use serde::Deserialize;

const INTERPRETER_PY: &str = include_str!("interpreter.py");

#[derive(Clone, Debug, Deserialize)]
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
    pub micro: u8,
    pub releaselevel: String,
    pub serial: u8,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Interpreter {
    pub path: PathBuf,
    pub realpath: PathBuf,
    pub prefix: PathBuf,
    pub base_prefix: Option<PathBuf>,
    pub version: PythonVersion,
    pub marker_env: MarkerEnvironment,
    pub macos_framework_build: bool,
    pub supported_tags: Vec<String>,
    pub has_ensurepip: bool,
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

impl Interpreter {
    #[time("debug", "Interpreter.{}")]
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

    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use rstest::rstest;
    use testing::venv_python_exe;
    use textwrap::dedent;

    use crate::Interpreter;

    #[rstest]
    fn test_tags_same_as_packaging(venv_python_exe: PathBuf) {
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

        let interpreter = Interpreter::load(venv_python_exe).unwrap();
        assert_eq!(expected_tags, interpreter.supported_tags);
    }
}
