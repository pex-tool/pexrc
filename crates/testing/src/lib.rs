// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

use anyhow::anyhow;
use ctor::dtor;
use fs_err as fs;
use python::{InterpreterIdentificationScript, Resources, embedded};
use rstest::fixture;
use target_lexicon::{HOST, OperatingSystem};

static TMP_DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub fn create_tmp_dir() -> PathBuf {
    let tmp_dir = tempfile::Builder::new()
        .prefix("pexrc-test-")
        .suffix(".dir")
        .tempdir()
        .unwrap()
        .keep();
    let mut tmp_dirs = TMP_DIRS.lock().unwrap();
    let chroot = tmp_dir.join("chroot");
    fs::create_dir(&chroot).unwrap();
    tmp_dirs.push(tmp_dir);
    chroot
}

#[dtor]
fn cleanup_tmp_dirs() {
    let tmp_dirs = TMP_DIRS.lock().unwrap();
    for tmp_dir in tmp_dirs.as_slice() {
        fs::remove_dir_all(tmp_dir).unwrap()
    }
}

#[fixture]
pub fn tmp_dir() -> PathBuf {
    create_tmp_dir()
}

static PEXRC_TESTING_CACHE_ROOT: LazyLock<anyhow::Result<PathBuf>> = LazyLock::new(|| {
    let cache_dir = cache::cache_dir("pexrc-dev", ".pexrc-dev")
        .ok_or_else(|| anyhow!("Failed to establish a testing cache dir."))?;
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
});

const MANAGED_PYTHON_VERSION: &str = "3.14.3";

#[fixture]
#[once]
pub fn python_exe() -> PathBuf {
    let install_dir = PEXRC_TESTING_CACHE_ROOT
        .as_ref()
        .unwrap()
        .join("interpreters");

    assert!(
        Command::new("uv")
            .args([
                "python",
                "install",
                "--managed-python",
                MANAGED_PYTHON_VERSION
            ])
            .env("UV_PYTHON_INSTALL_DIR", &install_dir)
            .spawn()
            .unwrap()
            .wait()
            .unwrap()
            .success()
    );

    let python_exe_bytes = Command::new("uv")
        .args(["python", "find", "--managed-python", MANAGED_PYTHON_VERSION])
        .env("UV_PYTHON_INSTALL_DIR", install_dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap()
        .stdout;

    PathBuf::from(String::from_utf8(python_exe_bytes).unwrap().trim())
}

#[fixture]
pub fn venv_python_exe(python_exe: &Path) -> PathBuf {
    let venv_dir = tmp_dir();
    let python_exe_basename = {
        Command::new(python_exe)
            .args(["-m", "venv"])
            .arg(&venv_dir)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        OsString::from(python_exe.file_name().unwrap())
    };

    if HOST.operating_system == OperatingSystem::Windows {
        venv_dir.join("Scripts")
    } else {
        venv_dir.join("bin")
    }
    .join(python_exe_basename)
}

#[fixture]
pub fn resources() -> impl Resources<'static> {
    embedded::RESOURCES
}

#[fixture]
pub fn interpreter_identification_script(
    mut resources: impl Resources<'static>,
) -> InterpreterIdentificationScript<'static> {
    InterpreterIdentificationScript::read(&mut resources).unwrap()
}
