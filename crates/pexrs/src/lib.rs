// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail};
use interpreter::Interpreter;
use log::info;
use logging_timer::time;
use pex::Pex;
use venv::{Virtualenv, populate};

#[time("debug", "{}")]
pub fn boot(
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path> + Sync + Send,
    argv: Vec<String>,
    gc: bool,
) -> anyhow::Result<i32> {
    let dst_dir = tempfile::Builder::new().disable_cleanup(!gc).tempdir()?;
    if !gc {
        info!("Will not gc: {path}", path = dst_dir.path().display())
    }
    let mut command = prepare_boot(dst_dir, python, python_args, pex, argv)?;
    exec(&mut command)
}

#[time("debug", "{}")]
fn prepare_boot(
    dst_dir: impl AsRef<Path>,
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path> + Sync + Send,
    argv: Vec<String>,
) -> anyhow::Result<Command> {
    info!(
        "boot({python}, {pex}, {argv:?})",
        python = python.as_ref().display(),
        pex = pex.as_ref().display(),
        argv = argv
    );

    let interpreter = Interpreter::load(&python)?;
    let pex = Pex::load(pex.as_ref())?;

    let venv = Virtualenv::create(&interpreter, dst_dir.as_ref(), false)?;
    populate(&venv, &pex)?;

    let mut command = Command::new(venv.interpreter.path);
    command.args(python_args).arg(dst_dir.as_ref()).args(argv);
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
pub fn mount(
    _python: impl AsRef<Path>,
    _pex: impl AsRef<Path> + Sync + Send,
) -> anyhow::Result<PathBuf> {
    bail!("TODO: XXX")
}
