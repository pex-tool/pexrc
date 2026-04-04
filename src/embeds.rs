// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use indexmap::IndexMap;

const EMBEDS_DIR: Dir<'static> = include_dir!("$EMBEDS_DIR");

pub(crate) static CLIBS_DIR: LazyLock<&'static Dir> =
    LazyLock::new(|| EMBEDS_DIR.get_dir("clibs").expect("Embeds include clibs/."));

pub static CLIB_BY_TARGET: LazyLock<IndexMap<&'static str, &'static Path>> =
    LazyLock::new(|| collect_embedded_files(&CLIBS_DIR));

pub(crate) static PROXIES_DIR: LazyLock<&'static Dir> = LazyLock::new(|| {
    EMBEDS_DIR
        .get_dir("proxies")
        .expect("Embeds include proxies/.")
});

pub static PROXY_BY_TARGET: LazyLock<IndexMap<&'static str, &'static Path>> =
    LazyLock::new(|| collect_embedded_files(&PROXIES_DIR));

fn collect_embedded_files(dir: &'static Dir) -> IndexMap<&'static str, &'static Path> {
    dir.files()
        .map(|file| {
            let path = file.path();
            let target = path
                .file_prefix()
                .expect("Embeds all have a file name with an extension")
                .to_str()
                .expect("Embed file names are utf-8 strings.");
            (target, path)
        })
        .collect()
}
