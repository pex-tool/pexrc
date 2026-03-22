// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::env;

use itertools::Itertools;
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

    pub fn python_identifier(&self) -> Cow<'_, str> {
        match self {
            Target::Windows(target) => {
                // N.B.: The last element of the "target-triple" (the 4th element) is msvc or gnu
                // depending on whether this was a native build or cross-build. Either way, the dll
                // can be loaded by the host Python interpreter; so we store the dll without the
                // C-lib component.
                Cow::Owned(target.split("-").take(3).join("-"))
            }
            Target::Apple(target) | Target::Unix(target) => Cow::Borrowed(target),
            Target::GnuLinux(linux) => Cow::Borrowed(linux.target),
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
    pub fn parse(targets: impl Iterator<Item = &'a str>, glibc: &'a Glibc<'a>) -> Self {
        let (xwin_targets, zigbuild_targets) = targets
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
        Self {
            // TODO: Resolve cargo xwin build issues or delete the cargo-xwin code paths.
            xwin_targets: vec![],
            zigbuild_targets: xwin_targets.into_iter().chain(zigbuild_targets).collect(),
        }
    }

    pub fn is_just_current(&'a self) -> anyhow::Result<Option<&'a str>> {
        let current_target = env::var("TARGET")?;
        let mut all_targets_iter = self.iter_all_targets();
        if let Some(target) = all_targets_iter.next()
            && target.as_str() == current_target
            && all_targets_iter.next().is_none()
        {
            Ok(Some(target.as_str()))
        } else {
            Ok(None)
        }
    }

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
        ClassifiedTargets::parse(self.targets.iter().copied(), glibc)
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
