// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::env;
use std::process::{Command, exit};

use python_proxy::read_python;

fn main() {
    let python = match read_python() {
        Ok(python) => python,
        Err(err) => {
            eprintln!("Failed to determine python executable path: {err}");
            exit(1);
        }
    };
    let mut command = Command::new(&python);
    command.args(env::args_os().skip(1));
    let lock = match cache::read_lock() {
        Ok(lock) => lock,
        Err(err) => {
            eprintln!("Failed to obtain PEXRC cache read lock: {err}");
            exit(1);
        }
    };
    match platform::exec(&mut command, &[lock]) {
        Ok(status) => exit(status),
        Err(err) => {
            eprintln!("Failed to spawn {python}: {err}", python = python.display());
            exit(1);
        }
    }
}
