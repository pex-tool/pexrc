// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

mod config;
mod downloads;
mod tools;

pub use config::Clib;

use crate::config::{CargoManifest, ClassifiedTargets, Glibc, RustToolchain};
use crate::tools::ToolBox;
pub use crate::tools::{FoundTool, InstallDirs, ToolInstallation, ToolInventory};

pub fn inventory_tools(
    cargo_manifest_contents: &str,
    install_dirs: InstallDirs,
) -> anyhow::Result<ToolInventory<'_>> {
    let build_config = {
        let cargo_manifest: CargoManifest = toml::from_str(cargo_manifest_contents)?;
        cargo_manifest.package.metadata.build
    };
    ToolBox::from(build_config).find_tools(install_dirs)
}

pub fn classify_targets<'a>(
    rust_toolchain_contents: &'a str,
    glibc: &'a Glibc,
) -> anyhow::Result<ClassifiedTargets<'a>> {
    let rust_toolchain: RustToolchain = toml::from_str(rust_toolchain_contents)?;
    Ok(rust_toolchain.toolchain.classify(glibc))
}
