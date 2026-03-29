// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

mod atomic;
mod fingerprint;
mod key;

use std::borrow::Cow;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::anyhow;
pub use atomic::{atomic_dir, atomic_file};
pub use fingerprint::{Fingerprint, HashOptions, fingerprint_file, hash_file};
pub use key::Key;
use logging_timer::time;

pub fn cache_dir(name: &str, alt_name: &str) -> Option<PathBuf> {
    if let Some(cache_dir) = dirs::cache_dir() {
        Some(cache_dir.join(name))
    } else {
        dirs::home_dir().map(|home_dir| home_dir.join(alt_name))
    }
}

static PEXRC_ROOT: LazyLock<Result<PathBuf, Cow<'static, str>>> = LazyLock::new(|| {
    if let Some(pexrc_root) = env::var_os("PEXRC_ROOT") {
        let path = Path::new(&pexrc_root);
        let components = path.components();
        let mut cache_dir = PathBuf::with_capacity(components.count() + 1);
        for component in path {
            if component == "~" {
                if let Some(home_dir) = dirs::home_dir() {
                    cache_dir.push(home_dir)
                } else {
                    return Err(Cow::Owned(format!(
                        "Failed to expand home dir in PEXRC_ROOT={pexrc_root:?}"
                    )));
                }
            } else {
                cache_dir.push(component)
            }
        }
        Ok(cache_dir.iter().collect())
    } else if let Some(cache_dir) = cache_dir("pexrc", ".pexrc") {
        Ok(cache_dir)
    } else {
        Err(Cow::Borrowed(
            "Failed to calculate a PEXRC_ROOT directory!\n\
            The PEXRC_ROOT environment variable was not set, an operating-system standard cache \
            dir could not be determined and the user's home directory could not be determined.",
        ))
    }
});

pub enum CacheDir {
    Interpreter,
    Venv,
}

impl CacheDir {
    pub fn root() -> anyhow::Result<&'static Path> {
        PEXRC_ROOT.as_deref().map_err(|err| anyhow!("{err}"))
    }

    fn version(&self) -> &'static str {
        match self {
            CacheDir::Interpreter => "0",
            CacheDir::Venv => "0",
        }
    }

    #[time("debug", "CacheDir.{}")]
    pub fn path(&self) -> anyhow::Result<PathBuf> {
        Self::root().map(|pexrc_root| {
            match self {
                CacheDir::Interpreter => pexrc_root.join("interpreters"),
                CacheDir::Venv => pexrc_root.join("venvs"),
            }
            .join(self.version())
        })
    }
}
