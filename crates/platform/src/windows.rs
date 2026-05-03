// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Command;

use is_executable::IsExecutable;

use crate::Perms;

pub fn symlink_or_link_or_copy(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    _relative: bool,
) -> io::Result<()> {
    crate::link_or_copy(src, dst)
}

pub fn is_executable(path: impl AsRef<Path>) -> io::Result<bool> {
    Ok(path.as_ref().is_executable())
}

pub const fn mark_executable(_file: &mut File) -> io::Result<()> {
    Ok(())
}

pub const fn set_permissions(_file: &mut File, _perms: Perms) -> io::Result<()> {
    Ok(())
}

pub fn path_as_bytes(path: &Path) -> io::Result<&[u8]> {
    crate::path_as_str(path).map(str::as_bytes)
}

pub fn exec(command: &mut Command, _files_to_keep_open: &[File]) -> io::Result<i32> {
    crate::spawn(command)
}
