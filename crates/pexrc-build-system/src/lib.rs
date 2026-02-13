// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

mod config;
mod downloads;
mod tools;

use std::ffi::OsStr;

pub use crate::config::{CargoBinstall, DownloadArchive};
use crate::config::{CargoManifest, ClassifiedTargets, Glibc, RustToolchain};
pub use crate::downloads::{ensure_download, ensure_downloads};
use crate::tools::ToolBox;
pub use crate::tools::{BinstallTool, FoundTool, ToolInventory, Zig, find_zig};

pub fn inventory_tools(
    cargo_manifest_contents: &str,
    search_path: impl AsRef<OsStr>,
) -> anyhow::Result<ToolInventory<'_>> {
    let build_config = {
        let cargo_manifest: CargoManifest = toml::from_str(cargo_manifest_contents)?;
        cargo_manifest.package.metadata.build
    };
    Ok(ToolBox::from(build_config).find_tools(&search_path))
}

pub fn classify_targets<'a>(
    rust_toolchain_contents: &'a str,
    glibc: &'a Glibc,
) -> anyhow::Result<ClassifiedTargets<'a>> {
    let rust_toolchain: RustToolchain = toml::from_str(rust_toolchain_contents)?;
    Ok(rust_toolchain.toolchain.classify(glibc))
}
