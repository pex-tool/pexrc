// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::borrow::Cow;
use std::fmt::{Display, Formatter};
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

    pub fn simplified_target_triple(&self) -> anyhow::Result<SimplifiedTarget> {
        let simplified_target_triple: Cow<'_, str> = match self {
            Target::Apple(_target) => Cow::Owned(format!("{arch}-macos", arch = self.arch())),
            Target::GnuLinux(target) | Target::Unix(target) => {
                if target.contains("-unknown-") {
                    Cow::Owned(target.replace("-unknown", ""))
                } else {
                    Cow::Borrowed(target)
                }
            }
            Target::Windows(_target) => Cow::Owned(format!("{arch}-windows", arch = self.arch())),
        };
        SimplifiedTarget::try_from(simplified_target_triple.as_ref())
    }

    pub fn fully_qualified_binary_name(
        &self,
        binary_name: &str,
        target_suffix: Option<&str>,
    ) -> anyhow::Result<String> {
        let triple = self.simplified_target_triple()?;
        let target_suffix = target_suffix.unwrap_or_default();
        Ok(match self {
            Target::Windows(_) => {
                format!("{binary_name}-{triple}{target_suffix}.exe")
            }
            _ => format!("{binary_name}-{triple}{target_suffix}"),
        })
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum SimplifiedTarget {
    Arm64LinuxGnu,
    Arm64LinuxMusl,
    Arm64Macos,
    Arm64Windows,
    Armv7LinuxGnuabihf,
    Ppc64leLinuxGnu,
    Riscv64gcLinuxGnu,
    S390xLinuxGnu,
    X64LinuxGnu,
    X64LinuxMusl,
    X64Macos,
    X64Windows,
}

impl SimplifiedTarget {
    pub fn try_from(value: impl AsRef<str>) -> anyhow::Result<Self> {
        Ok(match value.as_ref() {
            "aarch64-linux-gnu" => Self::Arm64LinuxGnu,
            "aarch64-linux-musl" => Self::Arm64LinuxMusl,
            "aarch64-macos" => Self::Arm64Macos,
            "aarch64-windows" => Self::Arm64Windows,
            "armv7-linux-gnueabihf" => Self::Armv7LinuxGnuabihf,
            "powerpc64le-linux-gnu" => Self::Ppc64leLinuxGnu,
            "riscv64gc-linux-gnu" => Self::Riscv64gcLinuxGnu,
            "s390x-linux-gnu" => Self::S390xLinuxGnu,
            "x86_64-linux-gnu" => Self::X64LinuxGnu,
            "x86_64-linux-musl" => Self::X64LinuxMusl,
            "x86_64-macos" => Self::X64Macos,
            "x86_64-windows" => Self::X64Windows,
            value => bail!("Not a supported simple platform: {value}"),
        })
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Arm64LinuxGnu => "aarch64-linux-gnu",
            Self::Arm64LinuxMusl => "aarch64-linux-musl",
            Self::Arm64Macos => "aarch64-macos",
            Self::Arm64Windows => "aarch64-windows",
            Self::Armv7LinuxGnuabihf => "armv7-linux-gnueabihf",
            Self::Ppc64leLinuxGnu => "powerpc64le-linux-gnu",
            Self::Riscv64gcLinuxGnu => "riscv64gc-linux-gnu",
            Self::S390xLinuxGnu => "s390x-linux-gnu",
            Self::X64LinuxGnu => "x86_64-linux-gnu",
            Self::X64LinuxMusl => "x86_64-linux-musl",
            Self::X64Macos => "x86_64-macos",
            Self::X64Windows => "x86_64-windows",
        }
    }
}

impl Display for SimplifiedTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.write_str(self.as_str())
    }
}
