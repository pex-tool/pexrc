// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
mod windows;

use std::ffi::OsStr;
use std::path::Path;

use anyhow::anyhow;
use fs_err as fs;
#[cfg(unix)]
pub use unix::{is_executable, mark_executable, path_as_bytes, symlink_or_link_or_copy};
#[cfg(windows)]
pub use windows::{is_executable, mark_executable, path_as_bytes, symlink_or_link_or_copy};

pub fn path_as_str(path: &Path) -> anyhow::Result<&str> {
    path.to_str().ok_or_else(|| {
        anyhow!(
            "Failed to convert non-UTF8 path to str: {path}",
            path = path.display()
        )
    })
}

pub fn os_str_as_str(text: &OsStr) -> anyhow::Result<&str> {
    text.to_str().ok_or_else(|| {
        anyhow!(
            "Failed to convert non-UTF8 text to str: {text}",
            text = text.display()
        )
    })
}

pub fn link_or_copy(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    fs::hard_link(&src, &dst)
        .or_else(|_| fs::copy(src, dst).map(|_| ()))
        .map_err(anyhow::Error::new)
}
