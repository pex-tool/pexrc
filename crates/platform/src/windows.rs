// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::fs::File;
use std::os::windows::fs::symlink_file;
use std::path::Path;

use anyhow::anyhow;

pub fn link_or_copy(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    fs::hard_link(src, dst)
        .or_else(|| fs::copy(src, dst))
        .map_err(anyhow::Error::new)
}

pub fn mark_executable(file: &mut File) -> anyhow::Result<()> {
    Ok(())
}

pub fn path_as_bytes(path: &Path) -> anyhow::Result<&[u8]> {
    crate::path_as_str(path).as_bytes()
}
