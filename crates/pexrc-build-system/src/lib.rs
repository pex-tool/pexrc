// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

mod config;
mod downloads;
mod tools;

use std::path::Path;

pub use config::Clib;

use crate::config::{CargoManifest, ClassifiedTargets, Glibc, RustToolchain};
use crate::tools::ToolBox;
pub use crate::tools::{FoundTool, InstallDirs, ToolInstallation};

pub fn ensure_tools_installed<'a>(
    cargo: &Path,
    cargo_manifest_contents: &'a str,
    install_dirs: InstallDirs,
    install_missing_tools: bool,
) -> anyhow::Result<ToolInstallation<'a>> {
    let cargo_manifest: CargoManifest = toml::from_str(cargo_manifest_contents)?;
    let tool_box: ToolBox = cargo_manifest.package.metadata.build.into();
    let tool_inventory = tool_box.find_tools(install_dirs)?;
    tool_inventory.ensure_tools_installed(cargo, install_missing_tools)
}

pub fn classify_targets<'a>(
    rust_toolchain_contents: &'a str,
    glibc: &'a Glibc,
) -> anyhow::Result<ClassifiedTargets<'a>> {
    let rust_toolchain: RustToolchain = toml::from_str(rust_toolchain_contents)?;
    Ok(rust_toolchain.toolchain.classify(glibc))
}
