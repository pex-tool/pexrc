// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::borrow::Cow;
use std::sync::LazyLock;

use anyhow::bail;
use target_lexicon::HOST;

pub enum Target<'a> {
    Apple(&'a str),
    GnuLinux(&'a str),
    Unix(&'a str),
    Windows(&'a str),
}

pub static CURRENT_TARGET: LazyLock<String> = LazyLock::new(|| HOST.to_string());

impl<'a> Target<'a> {
    pub fn current() -> anyhow::Result<Self> {
        Self::classify(CURRENT_TARGET.as_str())
    }

    pub fn classify(target: &'a str) -> anyhow::Result<Target<'a>> {
        if target.contains("-apple-") {
            Ok(Target::Apple(target))
        } else if target.contains("-linux-") {
            if target.ends_with("-gnu") {
                Ok(Target::GnuLinux(target))
            } else {
                Ok(Target::Unix(target))
            }
        } else if target.contains("-windows-") {
            Ok(Target::Windows(target))
        } else {
            bail!("The given target is not supported: {target}")
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Target::Apple(target)
            | Target::GnuLinux(target)
            | Target::Unix(target)
            | Target::Windows(target) => target,
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

    fn arch(&'a self) -> &'a str {
        self.as_str()
            .split("-")
            .next()
            .expect("Target triples always have a leading arch component.")
    }

    pub fn simplified_target_triple(&self) -> Cow<'a, str> {
        match self {
            Target::Apple(_target) => Cow::Owned(format!("{arch}-macos", arch = self.arch())),
            Target::GnuLinux(target) | Target::Unix(target) => {
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
            Target::Windows(_) => {
                format!("{binary_name}-{triple}{target_suffix}.exe")
            }
            _ => format!("{binary_name}-{triple}{target_suffix}"),
        }
    }
}
