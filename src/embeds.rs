// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::iter::Iterator;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::anyhow;
use include_dir::{Dir, include_dir};
use indexmap::IndexMap;
use target::SimplifiedTarget;

pub struct Binary<'a> {
    pub target: SimplifiedTarget,
    pub path: &'a Path,
    pub contents: &'a [u8],
}

const EMBEDS_DIR: Dir<'static> = include_dir!("$EMBEDS_DIR");

pub(crate) static CLIBS_DIR: LazyLock<&'static Dir> =
    LazyLock::new(|| EMBEDS_DIR.get_dir("clibs").expect("Embeds include clibs/."));

pub static CLIB_BY_TARGET: LazyLock<IndexMap<SimplifiedTarget, Binary<'static>>> =
    LazyLock::new(|| {
        CLIBS_DIR
            .files()
            .map(|file| {
                let path = file.path();
                let target = path
                    .file_prefix()
                    .expect("The C libraries all have a file name with an extension.")
                    .to_str()
                    .expect("The C library file names are utf-8 strings.");
                let target = SimplifiedTarget::try_from(target)
                    .expect("The C library file names are all derived from simplified targets.");
                (
                    target,
                    Binary {
                        target,
                        path,
                        contents: file.contents(),
                    },
                )
            })
            .collect()
    });

pub(crate) static PROXIES_DIR: LazyLock<&'static Dir> = LazyLock::new(|| {
    EMBEDS_DIR
        .get_dir("proxies")
        .expect("Embeds include proxies/.")
});

pub static PROXY_BY_TARGET: LazyLock<IndexMap<SimplifiedTarget, Binary<'static>>> =
    LazyLock::new(|| {
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
                let target = SimplifiedTarget::try_from(target)
                    .expect("The Python proxy file names are all derived from simplified targets.");
                (
                    target,
                    Binary {
                        target,
                        path,
                        contents: file.contents(),
                    },
                )
            })
            .collect()
    });

pub fn read_proxy_content(target: SimplifiedTarget) -> anyhow::Result<impl Read> {
    let proxy = PROXY_BY_TARGET
        .get(&target)
        .ok_or_else(|| anyhow!("There is no python-proxy for {target}"))?;
    return Ok(zstd::Decoder::new(proxy.contents)?);
}
