// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use ctor::dtor;
use rstest::fixture;
use target_lexicon::{OperatingSystem, Triple};

static TMP_DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub fn create_tmp_dir() -> PathBuf {
    let tmp_dir = tempfile::tempdir().unwrap().keep();
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

#[fixture]
#[once]
pub fn python_exe() -> PathBuf {
    let python_exe_bytes = Command::new("uv")
        .args(["python", "find"])
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

    if Triple::host().operating_system == OperatingSystem::Windows {
        venv_dir.join("Scripts")
    } else {
        venv_dir.join("bin")
    }
    .join(python_exe_basename)
}
