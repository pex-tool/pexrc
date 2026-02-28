// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

pub fn link_or_copy(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    symlink(src, dst).map_err(anyhow::Error::new)
}

pub fn mark_executable(file: &mut File) -> anyhow::Result<()> {
    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o755);
    file.set_permissions(permissions)?;
    Ok(())
}

pub fn path_as_bytes(path: &Path) -> anyhow::Result<&[u8]> {
    Ok(path.as_os_str().as_bytes())
}
