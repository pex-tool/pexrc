// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

mod downloads;
mod metadata;
mod rust_toolchain;
mod tools;

use std::path::Path;

pub use metadata::Clib;
use rust_toolchain::{ClassifiedTargets, Toolchain, parse_toolchain};

use crate::metadata::{Glibc, Metadata, parse_metadata};
use crate::tools::ToolBox;
pub use crate::tools::{FoundTool, InstallDirs, ToolInstallation};

pub fn ensure_tools_installed<'a>(
    cargo: &Path,
    cargo_manifest_contents: &'a str,
    install_dirs: InstallDirs,
    install_missing_tools: bool,
) -> anyhow::Result<ToolInstallation<'a>> {
    let metadata: Metadata = parse_metadata(cargo_manifest_contents)?;
    let tool_box = ToolBox::from(metadata.build);
    let tool_inventory = tool_box.find_tools(install_dirs)?;
    tool_inventory.ensure_tools_installed(cargo, install_missing_tools)
}

pub fn classify_targets<'a>(
    rust_toolchain_contents: &'a str,
    glibc: &'a Glibc,
) -> anyhow::Result<ClassifiedTargets<'a>> {
    let toolchain: Toolchain = parse_toolchain(rust_toolchain_contents)?;
    Ok(toolchain.classify_targets(glibc))
}
