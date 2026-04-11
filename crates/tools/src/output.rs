// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::io::Write;
use std::path::Path;

use fs_err::File;

pub(crate) enum Output {
    File(File),
    Stdout(io::Stdout),
}

impl Output {
    pub(crate) fn new(file: Option<&Path>) -> anyhow::Result<Self> {
        Ok(if let Some(path) = file {
            Self::File(File::create(path)?)
        } else {
            Self::Stdout(io::stdout())
        })
    }
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Output::File(file) => file.write(buf),
            Output::Stdout(stdout) => stdout.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Output::File(file) => file.flush(),
            Output::Stdout(stdout) => stdout.flush(),
        }
    }
}
