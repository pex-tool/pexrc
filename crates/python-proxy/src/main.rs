// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(string_from_utf8_lossy_owned)]

use std::fs::File;
use std::io::{BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Command, exit};
use std::{env, io};

use python_proxy::SHEBANG_PREFIX;

const PATH_MAX: usize = 4096;

struct PythonProxy {
    proxy: PathBuf,
    target: PathBuf,
    has_script: bool,
}

fn read_proxy() -> io::Result<PythonProxy> {
    let mut buf = vec![0u8; PATH_MAX];
    let proxy = proxy_path()?;
    let mut exe_fp = BufReader::new(File::open(&proxy)?);
    exe_fp.seek(SeekFrom::End(-(buf.len() as i64)))?;
    exe_fp.read_to_end(&mut buf)?;
    match buf
        .windows(SHEBANG_PREFIX.len())
        .rposition(|chunk| SHEBANG_PREFIX.as_bytes() == chunk)
    {
        Some(index) => {
            const EOCD_MAGIC: &[u8] = b"PK\x05\x06";
            let eocd_start = index - 22;
            let has_script = &buf[eocd_start..(eocd_start + EOCD_MAGIC.len())] == EOCD_MAGIC;
            buf.drain(..index + SHEBANG_PREFIX.len());
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
            Ok(PythonProxy {
                proxy,
                target,
                has_script,
            })
        }
        None => Err(io::Error::new(
            ErrorKind::NotFound,
            "Failed to find Python shebang footer.",
        )),
    }
}

#[cfg(unix)]
fn proxy_path() -> io::Result<PathBuf> {
    env::args()
        .next()
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "No argv0 was present; python-proxy cannot run.",
            )
        })
        .map(PathBuf::from)
}

#[cfg(windows)]
fn proxy_path() -> io::Result<PathBuf> {
    env::current_exe()
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
    if python_proxy.has_script {
        command.arg(python_proxy.proxy.as_os_str());
    }
    command.args(env::args_os().skip(1));
    command.env("__PYVENV_LAUNCHER__", &python_proxy.proxy);

    // N.B.: For Mac Python Framework builds (and Windows Python builds) __PYVENV_LAUNCHER__ is
    // deleted from the env on launch. We need to know about the launcher in the venv `pex` script;
    // so we duplicate that knowledge in our own env var.
    command.env("__PEXRC_PYVENV_LAUNCHER__", &python_proxy.proxy);

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
