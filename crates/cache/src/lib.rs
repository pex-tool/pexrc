// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

mod atomic;
mod fingerprint;

use std::borrow::Cow;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::anyhow;
pub use atomic::atomic_file;
pub use fingerprint::{Fingerprint, hash_file};

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
    } else if let Some(cache_dir) = dirs::cache_dir() {
        Ok(cache_dir.join("pexrc"))
    } else if let Some(home_dir) = dirs::home_dir() {
        Ok(home_dir.join(".pexrc"))
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
    fn version(&self) -> &'static str {
        match self {
            CacheDir::Interpreter => "0",
            CacheDir::Venv => "0",
        }
    }

    pub fn path(&self) -> anyhow::Result<PathBuf> {
        PEXRC_ROOT
            .as_ref()
            .map(|pexrc_root| {
                match self {
                    CacheDir::Interpreter => pexrc_root.join("interpreters"),
                    CacheDir::Venv => pexrc_root.join("venvs"),
                }
                .join(self.version())
            })
            .map_err(|err| anyhow!("{err}"))
    }
}
