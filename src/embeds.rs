// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::iter::Iterator;
use std::path::Path;
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use indexmap::IndexMap;

const EMBEDS_DIR: Dir<'static> = include_dir!("$EMBEDS_DIR");

pub(crate) static CLIBS_DIR: LazyLock<&'static Dir> =
    LazyLock::new(|| EMBEDS_DIR.get_dir("clibs").expect("Embeds include clibs/."));

pub static CLIB_BY_TARGET: LazyLock<IndexMap<&'static str, &'static Path>> = LazyLock::new(|| {
    CLIBS_DIR
        .files()
        .map(|file| {
            let path = file.path();
            let target = path
                .file_prefix()
                .expect("The C libraries all have a file name with an extension.")
                .to_str()
                .expect("The C library file names are utf-8 strings.");
            (target, path)
        })
        .collect()
});

pub(crate) static PROXIES_DIR: LazyLock<&'static Dir> = LazyLock::new(|| {
    EMBEDS_DIR
        .get_dir("proxies")
        .expect("Embeds include proxies/.")
});

pub static PROXY_BY_TARGET: LazyLock<IndexMap<&'static str, &'static Path>> = LazyLock::new(|| {
    PROXIES_DIR
        .files()
        .map(|file| {
            let path = file.path();
            let target = path
                .file_stem()
                .expect("The Python proxies all have a file name.")
                .to_str()
                .expect("The Python proxy file names are utf-8 strings.")
                .splitn(3, "-")
                .nth(2)
                .expect(
                    "The Python proxy file names are all of the form `python-proxy-<target>(.exe)?",
                );
            (target, path)
        })
        .collect()
});
