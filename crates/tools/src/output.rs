// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::io::Write;
use std::path::Path;

use fs_err::File;

pub(crate) fn using(
    output: Option<&Path>,
    func: impl Fn(Box<dyn Write>) -> io::Result<()>,
) -> anyhow::Result<()> {
    if let Some(path) = output {
        let file = Box::new(File::create(path)?);
        func(file)?
    } else {
        let stdout = Box::new(io::stdout());
        func(stdout)?
    }
    Ok(())
}
