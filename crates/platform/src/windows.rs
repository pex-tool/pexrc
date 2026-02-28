// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::fs::File;
use std::path::Path;

pub fn link_or_copy(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    fs::hard_link(&src, &dst)
        .or_else(|_| fs::copy(src, dst).map(|_| ()))
        .map_err(anyhow::Error::new)
}

pub fn mark_executable(_file: &mut File) -> anyhow::Result<()> {
    Ok(())
}

pub fn path_as_bytes(path: &Path) -> anyhow::Result<&[u8]> {
    crate::path_as_str(path).map(str::as_bytes)
}
