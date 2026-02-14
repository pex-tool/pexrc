// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::metadata::Glibc;

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
    pub(crate) fn classify_targets(&self, glibc: &'a Glibc<'a>) -> ClassifiedTargets<'a> {
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
struct RustToolchain<'a> {
    #[serde(borrow)]
    toolchain: Toolchain<'a>,
}

pub(crate) fn parse_toolchain(rust_toolchain_contents: &str) -> anyhow::Result<Toolchain<'_>> {
    let rust_toolchain: RustToolchain = toml::from_str(rust_toolchain_contents)?;
    Ok(rust_toolchain.toolchain)
}
