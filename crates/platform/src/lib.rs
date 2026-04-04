// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

// N.B.: We stick to stdlib as much as possible in this crate since it is re-used by the
// python-proxy crate which produces a size-sensitive binary. Particularly, no fs-err or anyhow: we
// handle ensuring useful errors manually in this crate.

#![deny(clippy::all)]

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
mod windows;

use std::ffi::OsStr;
use std::path::Path;
use std::{fs, io};

#[cfg(unix)]
pub use unix::{exec, is_executable, mark_executable, path_as_bytes, symlink_or_link_or_copy};
#[cfg(windows)]
pub use windows::{exec, is_executable, mark_executable, path_as_bytes, symlink_or_link_or_copy};

pub fn path_as_str(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::other(format!(
            "Failed to convert non-UTF8 path to str: {path}",
            path = path.display()
        ))
    })
}

pub fn os_str_as_str(text: &OsStr) -> io::Result<&str> {
    text.to_str().ok_or_else(|| {
        io::Error::other(format!(
            "Failed to convert non-UTF8 text to str: {text}",
            text = text.display()
        ))
    })
}

pub fn link_or_copy(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    fs::hard_link(src.as_ref(), dst.as_ref())
        .or_else(|_| fs::copy(src.as_ref(), dst.as_ref()).map(|_| ()))
        .map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "Failed to link or copy {src} -> {dst}: {err}",
                    src = src.as_ref().display(),
                    dst = dst.as_ref().display()
                ),
            )
        })
}
