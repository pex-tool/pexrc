// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{anyhow, bail};
use nix::errno::Errno;
use nix::unistd;
use nix::unistd::AccessFlags;

pub fn symlink_or_link_or_copy(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    relative: bool,
) -> anyhow::Result<()> {
    symlink(src, dst, relative)
}

pub fn symlink(src: impl AsRef<Path>, dst: impl AsRef<Path>, relative: bool) -> anyhow::Result<()> {
    let src = if relative
        && let Some(rel_base) = dst.as_ref().parent()
        && let Some(rel_path) = pathdiff::diff_paths(src.as_ref(), rel_base)
    {
        Cow::Owned(rel_path)
    } else {
        Cow::Borrowed(src.as_ref())
    };
    std::os::unix::fs::symlink(src, dst).map_err(anyhow::Error::new)
}

pub fn is_executable(path: impl AsRef<Path>) -> anyhow::Result<bool> {
    if !path
        .as_ref()
        .metadata()
        .map_err(|err| {
            anyhow!(
                "Failed to determine if {path} is a file: {err}",
                path = path.as_ref().display()
            )
        })?
        .is_file()
    {
        return Ok(false);
    }
    match unistd::access(path.as_ref(), AccessFlags::X_OK) {
        Ok(()) => Ok(true),
        Err(errno) => {
            if errno == Errno::EACCES {
                Ok(false)
            } else {
                bail!(
                    "Failed to determine access mode bits for {path}: errno {errno}: {desc}",
                    path = path.as_ref().display(),
                    desc = errno.desc()
                )
            }
        }
    }
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
