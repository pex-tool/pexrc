// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::bail;
use serde::Deserialize;

pub struct GnuLinux<'a> {
    target: &'a str,
    zigbuild_target: String,
}

pub enum Target<'a> {
    Apple(&'a str),
    GnuLinux(GnuLinux<'a>),
    Unix(&'a str),
    Windows(&'a str),
}

impl<'a> Target<'a> {
    pub fn as_str(&self) -> &str {
        match self {
            Target::Apple(target) | Target::Unix(target) | Target::Windows(target) => target,
            Target::GnuLinux(linux) => linux.target,
        }
    }

    pub fn shared_library_name(&self, lib_name: &str) -> String {
        match self {
            Target::Apple(_) => format!("lib{lib_name}.dylib"),
            Target::GnuLinux(_) | Target::Unix(_) => format!("lib{lib_name}.so"),
            Target::Windows(_) => format!("{lib_name}.dll"),
        }
    }
}

pub struct ClassifiedTargets<'a> {
    xwin_targets: Vec<Target<'a>>,
    zigbuild_targets: Vec<Target<'a>>,
}

impl<'a> ClassifiedTargets<'a> {
    pub fn iter_zigbuild_targets(&'a self) -> impl Iterator<Item = &'a str> {
        self.zigbuild_targets.iter().map(|target| {
            if let Target::GnuLinux(gnu_linux) = target {
                gnu_linux.zigbuild_target.as_str()
            } else {
                target.as_str()
            }
        })
    }

    pub fn iter_xwin_targets(&'a self) -> impl Iterator<Item = &'a str> {
        self.xwin_targets.iter().map(Target::as_str)
    }

    pub fn iter_all_targets(&'a self) -> impl Iterator<Item = &'a Target<'a>> {
        self.zigbuild_targets.iter().chain(self.xwin_targets.iter())
    }
}

#[derive(Deserialize)]
pub(crate) struct Toolchain<'a> {
    #[serde(borrow)]
    targets: Vec<&'a str>,
}

impl<'a> Toolchain<'a> {
    pub(crate) fn classify(&self, glibc: &'a Glibc<'a>) -> ClassifiedTargets<'a> {
        let (xwin_targets, zigbuild_targets) = self
            .targets
            .iter()
            .map(|target| {
                if target.contains("-apple-") {
                    Target::Apple(target)
                } else if target.contains("-linux-") {
                    if target.ends_with("-gnu") {
                        let zigbuild_target = format!(
                            "{target}.{glibc_version}",
                            glibc_version = glibc.version(target)
                        );
                        Target::GnuLinux(GnuLinux {
                            target,
                            zigbuild_target,
                        })
                    } else {
                        Target::Unix(target)
                    }
                } else if target.contains("-windows-") {
                    Target::Windows(target)
                } else {
                    panic!("The build system does not know how to handle")
                }
            })
            .partition::<Vec<_>, _>(|target| matches!(target, Target::Windows(_)));
        ClassifiedTargets {
            xwin_targets,
            zigbuild_targets,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct RustToolchain<'a> {
    #[serde(borrow)]
    pub(crate) toolchain: Toolchain<'a>,
}

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
    fn version(&self, target: &str) -> &str {
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
pub(crate) struct Package<'a> {
    #[serde(borrow)]
    pub(crate) metadata: Metadata<'a>,
}

#[derive(Deserialize)]
pub(crate) struct CargoManifest<'a> {
    #[serde(borrow)]
    pub(crate) package: Package<'a>,
}
