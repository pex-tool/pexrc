// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::bail;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Fingerprint<'a> {
    pub algorithm: &'a str,
    pub hash: &'a str,
}

#[derive(Deserialize, Debug)]
pub struct DownloadArchive<'a> {
    pub url: Cow<'a, str>,
    pub size: u64,
    #[serde(borrow)]
    pub fingerprint: Fingerprint<'a>,
    #[serde(default)]
    pub prefix: Option<&'a str>,
}

#[derive(Deserialize)]
pub struct Glibc<'a> {
    default_version: &'a str,
    by_platform: HashMap<&'a str, &'a str>,
}

impl<'a> Glibc<'a> {
    pub(crate) fn version(&self, target: &str) -> &str {
        self.by_platform
            .get(target)
            .map_or(self.default_version, |target| target)
    }
}

#[derive(Deserialize)]
pub struct Clib<'a> {
    pub profile: &'a str,
    pub compression_level: i32,
}

#[derive(Deserialize)]
pub struct Artifact<'a> {
    target: &'a str,
    #[serde(rename = "type")]
    archive_type: &'a str,
    size: u64,
    hash: &'a str,
}

#[derive(Deserialize)]
pub struct CargoBinstall<'a> {
    pub version: &'a str,
    pub artifacts: Vec<Artifact<'a>>,
}

impl<'a> CargoBinstall<'a> {
    pub fn download_for(&'a self, target: &'a str) -> anyhow::Result<Option<DownloadArchive<'a>>> {
        for artifact in &self.artifacts {
            if artifact.target != target {
                continue;
            }
            let (algorithm, hash) = if let Some(idx) = artifact.hash.find(":") {
                (&artifact.hash[0..idx], &artifact.hash[idx + 1..])
            } else {
                bail!(
                    "Invalid hash {hash} for cargo-binstall target {target}.\n\
                    Must be of form <algorithm>:<hex digest>",
                    hash = artifact.hash
                );
            };
            return Ok(Some(DownloadArchive {
                url: Cow::Owned(format!(
                    "https://github.com/cargo-bins/cargo-binstall/releases/download/\
                        v{version}/\
                        cargo-binstall-{target}.{ext}",
                    version = self.version,
                    ext = artifact.archive_type
                )),
                size: artifact.size,
                fingerprint: Fingerprint { algorithm, hash },
                prefix: None,
            }));
        }
        Ok(None)
    }
}

#[derive(Deserialize)]
pub struct Build<'a> {
    #[serde(borrow)]
    pub cargo_binstall: CargoBinstall<'a>,
    #[serde(borrow)]
    pub clib: Clib<'a>,
    #[serde(borrow)]
    pub glibc: Glibc<'a>,
    #[serde(borrow)]
    pub mac_osx_sdk: DownloadArchive<'a>,
    pub zig_version: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct Metadata<'a> {
    #[serde(borrow)]
    pub(crate) build: Build<'a>,
}

#[derive(Deserialize)]
struct Package<'a> {
    #[serde(borrow)]
    metadata: Metadata<'a>,
}

#[derive(Deserialize)]
struct CargoManifest<'a> {
    #[serde(borrow)]
    package: Package<'a>,
}

pub(crate) fn parse_metadata(cargo_manifest_contents: &str) -> anyhow::Result<Metadata<'_>> {
    let cargo_manifest: CargoManifest = toml::from_str(cargo_manifest_contents)?;
    Ok(cargo_manifest.package.metadata)
}
