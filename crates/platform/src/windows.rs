// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::path::Path;

use is_executable::IsExecutable;

pub fn symlink_or_link_or_copy(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    _relative: bool,
) -> anyhow::Result<()> {
    crate::link_or_copy(src, dst)
}

pub fn is_executable(path: impl AsRef<Path>) -> anyhow::Result<bool> {
    Ok(path.as_ref().is_executable())
}

pub fn mark_executable(_file: &mut File) -> anyhow::Result<()> {
    Ok(())
}

pub fn path_as_bytes(path: &Path) -> anyhow::Result<&[u8]> {
    crate::path_as_str(path).map(str::as_bytes)
}
