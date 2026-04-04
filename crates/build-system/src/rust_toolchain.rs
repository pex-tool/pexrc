// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::sync::LazyLock;

use serde::Deserialize;
use target_lexicon::HOST;

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

pub static CURRENT_TARGET: LazyLock<String> = LazyLock::new(|| HOST.to_string());

impl<'a> Target<'a> {
    pub fn current(glibc: &'a Glibc<'a>) -> Self {
        Self::classify(CURRENT_TARGET.as_str(), glibc)
    }

    pub fn classify(target: &'a str, glibc: &'a Glibc<'a>) -> Target<'a> {
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
    }

    pub fn as_str(&self) -> &str {
        match self {
            Target::Apple(target) | Target::Unix(target) | Target::Windows(target) => target,
            Target::GnuLinux(linux) => linux.target,
        }
    }

    pub fn zigbuild_target(&self) -> &str {
        match self {
            Target::Apple(target) | Target::Unix(target) | Target::Windows(target) => target,
            Target::GnuLinux(linux) => &linux.zigbuild_target,
        }
    }

    pub fn shared_library_name(&self, lib_name: &str) -> String {
        match self {
            Target::Apple(_) => format!("lib{lib_name}.dylib"),
            Target::GnuLinux(_) | Target::Unix(_) => format!("lib{lib_name}.so"),
            Target::Windows(_) => format!("{lib_name}.dll"),
        }
    }

    pub fn binary_name(&self, binary_name: &'a str, exe_suffix: Option<&str>) -> Cow<'a, str> {
        match self {
            Target::Windows(_) => Cow::Owned(format!(
                "{binary_name}{suffix}.exe",
                suffix = exe_suffix.unwrap_or_default()
            )),
            _ => {
                if let Some(suffix) = exe_suffix {
                    Cow::Owned(format!("{binary_name}{suffix}"))
                } else {
                    Cow::Borrowed(binary_name)
                }
            }
        }
    }

    fn arch(&self) -> &'a str {
        let target = match self {
            Target::Apple(target) | Target::Unix(target) | Target::Windows(target) => target,
            Target::GnuLinux(linux) => linux.target,
        };
        target
            .split("-")
            .next()
            .expect("Target triples always have a leading arch component.")
    }

    pub fn simplified_target_triple(&self) -> Cow<'a, str> {
        match self {
            Target::Apple(_target) => Cow::Owned(format!("{arch}-macos", arch = self.arch())),
            Target::GnuLinux(GnuLinux { target, .. }) | Target::Unix(target) => {
                if target.contains("-unknown-") {
                    Cow::Owned(target.replace("-unknown", ""))
                } else {
                    Cow::Borrowed(target)
                }
            }
            Target::Windows(_target) => Cow::Owned(format!("{arch}-windows", arch = self.arch())),
        }
    }

    pub fn fully_qualified_binary_name(
        &self,
        binary_name: &str,
        target_suffix: Option<&str>,
    ) -> String {
        let triple = self.simplified_target_triple();
        let target_suffix = target_suffix.unwrap_or_default();
        match self {
            Target::Windows(_target) => {
                format!("{binary_name}-{triple}{target_suffix}.exe")
            }
            _ => format!("{binary_name}-{triple}{target_suffix}"),
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
            .map(|target| Target::classify(target, glibc))
            .partition::<Vec<_>, _>(|target| matches!(target, Target::Windows(_)));
        Self {
            // TODO: Resolve cargo xwin build issues or delete the cargo-xwin code paths.
            xwin_targets: vec![],
            zigbuild_targets: xwin_targets.into_iter().chain(zigbuild_targets).collect(),
        }
    }

    pub fn iter_zigbuild_targets(&'a self) -> impl ExactSizeIterator<Item = &'a Target<'a>> {
        self.zigbuild_targets.iter()
    }

    pub fn iter_xwin_targets(&'a self) -> impl ExactSizeIterator<Item = &'a Target<'a>> {
        self.xwin_targets.iter()
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
    pub(crate) fn into_targets(self) -> Vec<String> {
        self.targets.into_iter().map(str::to_string).collect()
    }

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
