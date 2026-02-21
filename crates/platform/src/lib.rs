// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

use std::path::Path;

use anyhow::anyhow;
#[cfg(unix)]
pub use unix::{link_or_copy, mark_executable, path_as_bytes};
#[cfg(windows)]
pub use windows::{link_or_copy, mark_executable, path_as_bytes};

pub fn path_as_str(path: &Path) -> anyhow::Result<&str> {
    path.to_str().ok_or_else(|| {
        anyhow!(
            "Failed to convert non-UTF8 path to str: {path}",
            path = path.display()
        )
    })
}
