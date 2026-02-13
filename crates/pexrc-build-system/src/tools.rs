// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bstr::ByteSlice;
use strum::{EnumCount, IntoEnumIterator};
use strum_macros::{EnumCount, EnumIter};
use which::which_in_global;

use crate::config::{Build, CargoBinstall, Clib, DownloadArchive, Glibc};

#[derive(EnumCount, EnumIter)]
pub enum BinstallTool {
    CargoXwin,
    CargoZigbuild,
    Uv,
}

impl BinstallTool {
    pub fn binary_name(&self) -> &'static str {
        match *self {
            BinstallTool::CargoXwin => "cargo-xwin",
            BinstallTool::CargoZigbuild => "cargo-zigbuild",
            BinstallTool::Uv => "uv",
        }
    }
}

pub struct FoundTool {
    pub env_var: &'static str,
    pub path: PathBuf,
}

pub enum Zig<'a> {
    Found(FoundTool),
    MissingVersion(&'a str),
}

impl<'a> Zig<'a> {
    pub fn found(&self) -> bool {
        matches!(*self, Zig::Found(_))
    }

    pub fn missing_version(&'a self) -> Option<&'a str> {
        match *self {
            Zig::MissingVersion(version) => Some(version),
            _ => None,
        }
    }
}

pub struct ToolInventory<'a> {
    pub clib: Clib<'a>,
    pub binstall: CargoBinstall<'a>,
    pub zig: Zig<'a>,
    pub glibc: Glibc<'a>,
    pub downloads: Vec<(&'static str, DownloadArchive<'a>)>,
    pub missing: Vec<BinstallTool>,
}

pub(crate) struct ToolBox<'a> {
    clib: Clib<'a>,
    binstall: CargoBinstall<'a>,
    zig_version: &'a str,
    glibc: Glibc<'a>,
    binstall_tools: Vec<BinstallTool>,
    downloads: Vec<(&'static str, DownloadArchive<'a>)>,
}

impl<'a> From<Build<'a>> for ToolBox<'a> {
    fn from(build: Build<'a>) -> Self {
        Self {
            clib: build.clib,
            binstall: build.cargo_binstall,
            zig_version: build.zig_version,
            glibc: build.glibc,
            binstall_tools: BinstallTool::iter().collect::<Vec<_>>(),
            downloads: vec![("SDKROOT", build.mac_osx_sdk)],
        }
    }
}

impl<'a> ToolBox<'a> {
    pub(crate) fn find_tools(self, search_path: impl AsRef<OsStr>) -> ToolInventory<'a> {
        let mut missing: Vec<BinstallTool> = Vec::with_capacity(BinstallTool::COUNT);
        let zig = if let Some(zig) = find_zig(
            &["zig", "python-zig"],
            self.zig_version,
            search_path.as_ref(),
        ) {
            Zig::Found(zig)
        } else {
            Zig::MissingVersion(self.zig_version)
        };
        for tool in self.binstall_tools {
            if let Ok(Some(exe)) = which_in_global(tool.binary_name(), Some(&search_path))
                .map(|mut found| found.next())
            {
                eprintln!(
                    "Found {tool} at {exe}",
                    tool = tool.binary_name(),
                    exe = exe.display()
                );
            } else {
                missing.push(tool)
            }
        }
        ToolInventory {
            clib: self.clib,
            binstall: self.binstall,
            downloads: self.downloads,
            zig,
            glibc: self.glibc,
            missing,
        }
    }
}

pub fn find_zig(binary_names: &[&str], version: &str, search_path: &OsStr) -> Option<FoundTool> {
    for binary_name in binary_names {
        if let Ok(zig_paths) = which_in_global(binary_name, Some(search_path)) {
            for zig in zig_paths {
                if let Some(zig_version) = get_zig_version(&zig)
                    && zig_version == version
                {
                    return Some(FoundTool {
                        env_var: "CARGO_ZIGBUILD_ZIG_PATH",
                        path: zig,
                    });
                }
            }
        }
    }
    None
}

fn get_zig_version(zig: &Path) -> Option<String> {
    Command::new(zig)
        .arg("version")
        .stdout(Stdio::piped())
        .spawn()
        .ok()
        .and_then(|child| child.wait_with_output().ok())
        .and_then(|result| {
            if result.status.success() {
                result.stdout.to_str().ok().map(str::trim).map(String::from)
            } else {
                None
            }
        })
}
