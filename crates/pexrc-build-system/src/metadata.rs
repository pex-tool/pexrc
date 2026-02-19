// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::bail;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub(crate) struct Fingerprint<'a> {
    pub(crate) algorithm: &'a str,
    pub(crate) hash: &'a str,
}

#[derive(Deserialize, Debug)]
pub(crate) struct DownloadArchive<'a> {
    pub(crate) url: Cow<'a, str>,
    pub(crate) size: u64,
    #[serde(borrow)]
    pub(crate) fingerprint: Fingerprint<'a>,
    #[serde(default)]
    pub(crate) prefix: Option<&'a str>,
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
pub struct ClibConfiguration<'a> {
    pub profile: &'a str,
    pub compression_level: i32,
}

#[derive(Deserialize)]
pub struct Clib<'a> {
    pub compression_level: i32,
    #[serde(borrow)]
    profiles: HashMap<&'a str, ClibConfiguration<'a>>,
}

impl<'a> Clib<'a> {
    pub fn configuration_for(&'a mut self, profile: &'a str) -> &'a ClibConfiguration<'a> {
        self.profiles
            .entry(profile)
            .or_insert_with(|| ClibConfiguration {
                profile,
                compression_level: self.compression_level,
            })
    }
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
pub(crate) struct CargoBinstall<'a> {
    pub(crate) version: &'a str,
    pub(crate) artifacts: Vec<Artifact<'a>>,
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
pub(crate) struct Build<'a> {
    #[serde(borrow)]
    pub(crate) cargo_binstall: CargoBinstall<'a>,
    #[serde(borrow)]
    pub(crate) clib: Clib<'a>,
    #[serde(borrow)]
    pub(crate) glibc: Glibc<'a>,
    #[serde(borrow)]
    pub(crate) mac_osx_sdk: DownloadArchive<'a>,
    pub(crate) zig_version: &'a str,
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
