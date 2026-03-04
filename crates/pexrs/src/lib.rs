// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::anyhow;
use cache::{CacheDir, atomic_dir};
use itertools::Itertools;
use log::debug;
use logging_timer::time;
use pex::Pex;
use venv::{Virtualenv, populate};

#[time("debug", "{}")]
pub fn boot(
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path>,
    argv: Vec<String>,
) -> anyhow::Result<i32> {
    let mut command = prepare_boot(python, python_args, pex, argv)?;
    debug!(
        "Booting with {exe} {args}",
        exe = command.get_program().to_string_lossy(),
        args = command.get_args().map(OsStr::to_string_lossy).join(" ")
    );
    exec(&mut command)
}

#[time("debug", "{}")]
fn prepare_boot(
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path>,
    argv: Vec<String>,
) -> anyhow::Result<Command> {
    let venv = prepare_venv(python, pex.as_ref())?;
    let mut command = Command::new(&venv.interpreter.path);
    command
        .args(python_args)
        .arg(venv.prefix().as_os_str())
        .args(argv);
    Ok(command)
}

#[cfg(unix)]
fn exec(command: &mut Command) -> anyhow::Result<i32> {
    use std::os::unix::process::CommandExt;
    let err = command.exec();
    Err(anyhow!("Failed to exec {command:?}: {err}"))
}

#[cfg(windows)]
fn exec(command: &mut Command) -> anyhow::Result<i32> {
    command
        .spawn()
        .map_err(|err| anyhow!("Failed to spawn() {command:?}: {err}"))?
        .wait()
        .map(|exit_status| {
            exit_status
                .code()
                .unwrap_or_else(|| if exit_status.success() { 0 } else { 1 })
        })
        .map_err(|err| anyhow!("Failed to wait() for {command:?}: {err}"))
}

#[time("debug", "{}")]
pub fn mount(python: impl AsRef<Path>, pex: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    prepare_venv(python, pex.as_ref()).map(|venv| venv.site_packages_path())
}

#[time("debug", "{}")]
fn prepare_venv<'a>(python: impl AsRef<Path>, pex: &'a Path) -> anyhow::Result<Virtualenv<'a>> {
    let pex = Pex::load(pex)?;
    let pex_info = pex.info();
    let venv_dir = CacheDir::Venv.path()?.join(&pex_info.pex_hash);
    if let Some(venv_interpreter) = atomic_dir(&venv_dir, |work_dir| {
        // TODO: XXX: Account for PEX_PATH
        let (interpreter, selected_wheels, mut resources) = pex.resolve(Some(python.as_ref()))?;
        let venv = Virtualenv::create(
            &interpreter,
            Cow::Borrowed(work_dir),
            &mut resources,
            pex_info.venv_system_site_packages,
        )?;
        populate(&venv, &venv_dir, &pex, &selected_wheels, &mut resources)?;
        Ok(venv.interpreter)
    })? {
        debug!("Built venv at {path}", path = venv_dir.display());
        let venv_interpreter = Virtualenv::host_interpreter(&venv_dir, &venv_interpreter);
        venv_interpreter.store()?;
        Virtualenv::enclosing(venv_interpreter)
    } else {
        debug!("Loading cached venv at {path}", path = venv_dir.display());
        let mut resources = pex.resources()?;
        Virtualenv::load(Cow::Owned(venv_dir), &mut resources)
    }
}
