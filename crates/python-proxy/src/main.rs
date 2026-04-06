// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(string_from_utf8_lossy_owned)]

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Command, exit};
use std::{env, io};

const PATH_MAX: usize = 4096;

struct PythonProxy {
    proxy: PathBuf,
    target: PathBuf,
}

fn read_proxy() -> Result<PythonProxy, io::Error> {
    let mut buf = vec![0u8; PATH_MAX];
    let proxy = PathBuf::from(env::args().next().ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            "No argv0 was present; python-proxy cannot run.",
        )
    })?);
    let mut exe_fp = File::open(&proxy)?;
    exe_fp.seek(SeekFrom::End(-(buf.len() as i64)))?;
    exe_fp.read_to_end(&mut buf)?;
    match buf.windows(2).rposition(|chunk| b"#!" == chunk) {
        Some(index) => {
            buf.drain(..index + 2);
            buf.truncate(buf.trim_ascii_end().len());
            let target = String::from_utf8(buf).map(PathBuf::from).map_err(|err| {
                io::Error::new(
                    ErrorKind::InvalidFilename,
                    format!(
                        "Python shebang footer contained a non-UTF-8 path: {buf}",
                        buf = err.into_utf8_lossy()
                    ),
                )
            })?;
            Ok(PythonProxy { proxy, target })
        }
        None => Err(io::Error::new(
            ErrorKind::NotFound,
            "Failed to find Python shebang footer.",
        )),
    }
}

fn main() {
    let python_proxy = match read_proxy() {
        Ok(python) => python,
        Err(err) => {
            eprintln!("Failed to determine python executable path: {err}");
            exit(1);
        }
    };
    let mut command = if python_proxy.target.is_absolute() {
        Command::new(&python_proxy.target)
    } else if let Some(proxy_dir) = python_proxy.proxy.parent() {
        Command::new(proxy_dir.join(&python_proxy.target))
    } else {
        eprintln!(
            "The proxy target {target} is relative but the python-proxy at {proxy} has no parent \
            directory to base that relative path in",
            target = python_proxy.target.display(),
            proxy = python_proxy.proxy.display()
        );
        exit(1);
    };
    command.args(env::args_os().skip(1));
    command.env("__PYVENV_LAUNCHER__", &python_proxy.proxy);
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
            eprintln!(
                "Failed to spawn {python}: {err}",
                python = python_proxy.target.display()
            );
            exit(1);
        }
    }
}
