// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(string_from_utf8_lossy_owned)]

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::{env, io};

const PATH_MAX: usize = 4096;

pub fn read_python() -> Result<PathBuf, io::Error> {
    let mut buf = vec![0u8; PATH_MAX];
    let mut exe_fp = File::open(env::current_exe()?)?;
    exe_fp.seek(SeekFrom::End(-(buf.len() as i64)))?;
    exe_fp.read_to_end(&mut buf)?;
    match buf.windows(2).rposition(|chunk| b"#!" == chunk) {
        Some(index) => {
            buf.drain(..index + 2);
            buf.truncate(buf.trim_ascii_end().len());
            String::from_utf8(buf).map(PathBuf::from).map_err(|err| {
                io::Error::new(
                    ErrorKind::InvalidFilename,
                    format!(
                        "Python shebang footer contained a non-UTF-8 path: {buf}",
                        buf = err.into_utf8_lossy()
                    ),
                )
            })
        }
        None => Err(io::Error::new(
            ErrorKind::NotFound,
            "Failed to find Python shebang footer.",
        )),
    }
}
