// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use logging_timer::time;

pub struct SearchPath {
    python: Option<OsString>,
    path: Option<OsString>,
    binary_paths: Option<Vec<PathBuf>>,
}

impl SearchPath {
    #[time("debug", "SearchPath.{}")]
    pub fn from_env() -> anyhow::Result<Self> {
        let mut binary_paths: Option<Vec<PathBuf>> = None;
        let pex_python = env::var_os("PEX_PYTHON").and_then(|python| {
            let python = PathBuf::from(python);
            if python.is_absolute() && python.is_file() {
                binary_paths.get_or_insert_default().push(python);
                None
            } else {
                Some(python.into_os_string())
            }
        });
        let pex_python_path = if let Some(pex_python_path) = env::var_os("PEX_PYTHON_PATH") {
            Some(env::join_paths(
                env::split_paths(&pex_python_path).filter_map(|entry| {
                    if entry.is_absolute() && entry.is_file() {
                        binary_paths.get_or_insert_default().push(entry);
                        None
                    } else {
                        Some(entry)
                    }
                }),
            )?)
        } else {
            None
        };
        Ok(Self {
            python: pex_python,
            path: pex_python_path,
            binary_paths,
        })
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Option<OsString>,
        Option<OsString>,
        Option<impl Iterator<Item = PathBuf>>,
    ) {
        (
            self.python,
            self.path,
            self.binary_paths.map(Vec::into_iter),
        )
    }
}
