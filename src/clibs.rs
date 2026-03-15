// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use indexmap::IndexMap;

pub(crate) const CLIBS_DIR: Dir<'static> = include_dir!("$CLIBS_DIR");

pub static CLIB_BY_TARGET: LazyLock<IndexMap<&'static str, &'static Path>> = LazyLock::new(|| {
    CLIBS_DIR
        .files()
        .map(|file| {
            let path = file.path();
            let target = path
                .file_prefix()
                .expect("Embedded C-libs all have a file name with an extension")
                .to_str()
                .expect("Embedded C-lib file names are utf-8 strings.");
            (target, path)
        })
        .collect()
});
