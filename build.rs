// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, io};

use anyhow::bail;
use bstr::ByteSlice;
use itertools::Itertools;
use pexrc_build_system::{
    Clib,
    FoundTool,
    InstallDirs,
    ToolInstallation,
    classify_targets,
    ensure_tools_installed,
};

fn main() -> anyhow::Result<()> {
    println!("cargo::rerun-if-changed=crates");

    env_logger::init();

    let cargo: PathBuf = env::var("CARGO")?.into();

    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("target")
    };

    let install_dirs = InstallDirs::system("pexrc-dev").unwrap_or_else(|| {
        let cache_base_dir = target_dir.join(".pexrc-dev");
        println!(
            "cargo::warning=Failed to discover the user cache dir; using {cache_base_dir}",
            cache_base_dir = cache_base_dir.display()
        );
        InstallDirs::new(cache_base_dir)
    });

    let cargo_manifest_contents = {
        let manifest_path = env::var("CARGO_MANIFEST_PATH")?;
        println!("cargo::rerun-if-changed={manifest_path}");
        fs::read_to_string(manifest_path)?
    };

    println!("cargo::rerun-if-env-changed=PEXRC_INSTALL_TOOLS");
    let install_missing_tools = env::var_os("PEXRC_INSTALL_TOOLS").unwrap_or_default() == "1";

    let tool_installation = ensure_tools_installed(
        &cargo,
        &cargo_manifest_contents,
        install_dirs,
        install_missing_tools,
    )?;
    let (clib, glibc, found_tools) = match tool_installation {
        ToolInstallation::Success(results) => results,
        ToolInstallation::Failure((zig, missing_binstall_tools, tool_search_path)) => {
            bail!(
                "The following tools are required but are not installed: {tools}\n\
                Searched PATH: {search_path}\n\
                Re-run with PEXRC_INSTALL_TOOLS=1 to let the build script install these tools.",
                tools = missing_binstall_tools
                    .iter()
                    .map(|tool| Cow::Borrowed(tool.binary_name()))
                    .chain(
                        zig.missing_version()
                            .iter()
                            .map(|version| Cow::Owned(format!("zig@{version}")))
                    )
                    .join(" "),
                search_path = tool_search_path.display()
            );
        }
    };

    println!("cargo::rerun-if-changed=rust-toolchain");
    let rust_toolchain_contents = fs::read_to_string("rust-toolchain")?;
    let targets = classify_targets(&rust_toolchain_contents, &glibc)?;

    custom_cargo_build(
        &cargo,
        ["xwin", "build"],
        &clib,
        &found_tools,
        targets.iter_xwin_targets(),
    )?;
    custom_cargo_build(
        &cargo,
        ["zigbuild"],
        &clib,
        &found_tools,
        targets.iter_zigbuild_targets(),
    )?;

    let (clibs_dir, compress) = if let Some(lib_dir) = env::var_os("PEXRC_LIB_DIR") {
        (PathBuf::from(lib_dir), false)
    } else {
        let out_dir = env::var_os("OUT_DIR").unwrap();
        (PathBuf::from(out_dir).join("clibs"), true)
    };
    fs::create_dir_all(&clibs_dir)?;
    println!(
        "cargo::rustc-env=CLIBS_DIR={clibs_dir}",
        clibs_dir = clibs_dir.display()
    );

    for target in targets.iter_all_targets() {
        let clib_name = target.shared_library_name("pexrc");
        let clib_path = target_dir
            .join(target.as_str())
            .join(clib.profile)
            .join(&clib_name);
        if !clib_path.exists() {
            eprintln!(
                "The clib for {target} does not exist at {clib_path}!",
                clib_path = clib_path.display(),
                target = target.as_str(),
            );
        }
        let mut dst = File::create(
            clibs_dir.join(format!("{target}.{clib_name}", target = target.as_str())),
        )?;
        if compress {
            io::copy(
                &mut File::open(clib_path)?,
                &mut zstd::Encoder::new(dst, clib.compression_level)?,
            )?;
        } else {
            io::copy(&mut File::open(clib_path)?, &mut dst)?;
        }
    }

    Ok(())
}

fn custom_cargo_build<'a>(
    cargo: &Path,
    custom_build_args: impl IntoIterator<Item = &'a str>,
    clib: &Clib,
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
    cmd.args(custom_build_args)
        .args(["--package", "clib", "--profile", clib.profile]);
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
