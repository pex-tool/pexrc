// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, io};

use anyhow::bail;
use bstr::ByteSlice;
use itertools::Itertools;
use pexrc_build_system::{
    FoundTool,
    InstallDirs,
    ToolInventory,
    classify_targets,
    inventory_tools,
};

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cargo: PathBuf = env::var("CARGO")?.into();

    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("target")
    };

    let install_dirs = if let Some(cache_dir) = dirs::cache_dir() {
        InstallDirs::new(cache_dir.join("pexrc-dev"))
    } else {
        let cache_dir = target_dir.join(".pexrc-dev");
        println!(
            "cargo::warning=Failed to discover the user cache dir; using {cache_dir}",
            cache_dir = cache_dir.display()
        );
        InstallDirs::new(cache_dir)
    };

    let data = {
        let manifest_path = env::var("CARGO_MANIFEST_PATH")?;
        fs::read_to_string(manifest_path)?
    };
    let tool_inventory = inventory_tools(data.as_str(), install_dirs)?;
    let found_tools = tool_inventory.ensure_tools_installed(&cargo)?;

    let rust_toolchain_contents = fs::read_to_string("rust-toolchain")?;
    let targets = classify_targets(rust_toolchain_contents.as_str(), &tool_inventory.glibc)?;

    custom_cargo_build(
        &cargo,
        ["xwin", "build"],
        &tool_inventory,
        &found_tools,
        targets.iter_xwin_targets(),
    )?;
    custom_cargo_build(
        &cargo,
        ["zigbuild"],
        &tool_inventory,
        &found_tools,
        targets.iter_zigbuild_targets(),
    )?;

    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap().into();
    let clibs_dir = out_dir.join("clibs");
    fs::create_dir_all(&clibs_dir)?;
    println!(
        "cargo::rustc-env=CLIBS_DIR={clibs_dir}",
        clibs_dir = clibs_dir.display()
    );

    for target in targets.iter_all_targets() {
        let clib_name = target.shared_library_name("pexrc");
        let clib = target_dir
            .join(target.as_str())
            .join(tool_inventory.clib.profile)
            .join(&clib_name);
        if !clib.exists() {
            eprintln!(
                "The clib for {target} does not exist at {clib_path}!",
                clib_path = clib.display(),
                target = target.as_str(),
            );
        }
        io::copy(
            &mut File::open(clib)?,
            &mut zstd::Encoder::new(
                File::create(
                    clibs_dir.join(format!("{target}.{clib_name}", target = target.as_str())),
                )?,
                tool_inventory.clib.compression_level,
            )?,
        )?;
    }

    Ok(())
}

fn custom_cargo_build<'a>(
    cargo: &Path,
    custom_build_args: impl IntoIterator<Item = &'a str>,
    tool_inventory: &ToolInventory,
    found_tools: impl IntoIterator<Item = &'a FoundTool>,
    targets: impl Iterator<Item = &'a str>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new(cargo);
    let cmd = cmd
        .stderr(Stdio::piped())
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TERM_COLOR", "always");
    for found_tool in found_tools {
        println!(
            "cargo::rustc-env={env_var}={path}",
            env_var = found_tool.env_var,
            path = found_tool.path.display()
        );
        cmd.env(found_tool.env_var, &found_tool.path);
    }
    cmd.args(custom_build_args).args([
        "--package",
        "clib",
        "--profile",
        tool_inventory.clib.profile,
    ]);
    for target in targets {
        cmd.args(["--target", target]);
    }

    let result = cmd.spawn()?.wait_with_output()?;
    if result.status.success() {
        return Ok(());
    }
    bail!(
        "Failed to compile clib with exit code {exit_code}:\n{exe} \\\n  {args}\n{output}",
        exit_code = result.status,
        exe = cmd.get_program().to_string_lossy(),
        args = cmd.get_args().map(OsStr::to_string_lossy).join(" \\\n  "),
        output = result.stderr.to_str_lossy()
    )
}
