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
