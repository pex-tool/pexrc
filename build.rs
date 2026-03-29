// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, io, iter};

use anyhow::{anyhow, bail};
use bstr::ByteSlice;
use build_system::{
    ClassifiedTargets,
    ClibConfiguration,
    FoundTool,
    Target,
    classify_targets,
    ensure_tools_installed,
};
use fs_err as fs;
use fs_err::File;
use itertools::Itertools;

fn main() -> anyhow::Result<()> {
    println!("cargo::rerun-if-changed=crates");

    env_logger::init();

    let cargo: PathBuf = env::var("CARGO")?.into();
    let cargo_manifest_contents = {
        let manifest_path = env::var("CARGO_MANIFEST_PATH")?;
        println!("cargo::rerun-if-changed={manifest_path}");
        fs::read_to_string(manifest_path)?
    };
    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("target")
    };

    let (mut clib, glibc, found_tools) =
        ensure_tools_installed(&cargo, &cargo_manifest_contents, &target_dir, true)?;
    println!("cargo::rerun-if-env-changed=PROFILE");
    let profile = env::var("PROFILE")?;
    let clib = clib.configuration_for(&profile);

    println!("cargo::rerun-if-env-changed=PEXRC_TARGETS");
    let all_targets = match env::var("PEXRC_TARGETS")
        .ok()
        .as_deref()
        .map(str::to_ascii_lowercase)
    {
        Some(targets) if targets == "all" => true,
        Some(targets) if targets == "current" => false,
        Some(targets) => bail!(
            "Unrecognized custom targets `PEXRC_TARGETS={targets}`.\n\
            Only `current` (the default) and `all` are recognized."
        ),
        None => false,
    };

    let clibs_dir = {
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let clibs_dir = PathBuf::from(out_dir).join("clibs");
        if all_targets {
            clibs_dir.join("all")
        } else {
            clibs_dir.join("current")
        }
    };
    fs::create_dir_all(&clibs_dir)?;
    println!(
        "cargo::rustc-env=CLIBS_DIR={clibs_dir}",
        clibs_dir = clibs_dir.display()
    );

    // N.B.: We need to use a custom --target-dir to avoid a deadlock on the ambient target that
    // would otherwise occur calling into cargo build recursively below.
    let tgt_path = target_dir.join("clib");
    let tgt_arg = tgt_path.to_str().ok_or_else(|| {
        anyhow!(
            "The target directory of {target_dir} must be a UTF-8 path",
            target_dir = target_dir.display()
        )
    })?;

    println!("cargo::rerun-if-env-changed=PEXRC_TARGETS");
    if all_targets {
        println!("cargo::rerun-if-changed=rust-toolchain");
        let rust_toolchain_contents = fs::read_to_string("rust-toolchain")?;
        let targets = classify_targets(&rust_toolchain_contents, &glibc)?;
        custom_cargo_build(
            &cargo,
            &["zigbuild", "--target-dir", tgt_arg],
            clib.profile,
            &found_tools,
            targets.iter_zigbuild_targets().map(Target::zigbuild_target),
        )?;
        custom_cargo_build(
            &cargo,
            &["xwin", "build", "--target-dir", tgt_arg],
            clib.profile,
            &found_tools,
            targets.iter_xwin_targets().map(Target::as_str),
        )?;
        collect_clibs(&targets, &tgt_path, clib, &clibs_dir, true)
    } else {
        let target = env::var("TARGET")?;
        let targets = ClassifiedTargets::parse([target.as_str()].into_iter(), &glibc);
        custom_cargo_build(
            &cargo,
            &["build", "--target-dir", tgt_arg],
            clib.profile,
            &found_tools,
            iter::empty(),
        )?;
        collect_clibs(&targets, &tgt_path, clib, &clibs_dir, true)
    }
}

fn custom_cargo_build<'a>(
    cargo: &Path,
    custom_build_args: &[&str],
    profile: &str,
    found_tools: &[FoundTool],
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
    cmd.args(custom_build_args).args(["--package", "clib"]);
    cmd.args([
        "--profile",
        if profile == "debug" { "dev" } else { profile },
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

fn collect_clibs<'a>(
    targets: &'a ClassifiedTargets<'a>,
    target_dir: &Path,
    clib: &'a ClibConfiguration<'a>,
    clibs_dir: &Path,
    compress: bool,
) -> anyhow::Result<()> {
    let is_just_current_target = targets.is_just_current()?;
    for target in targets.iter_all_targets() {
        let clib_name = target.shared_library_name("pexrc");
        let clib_path = if is_just_current_target.is_some() {
            target_dir.join(clib.profile).join("deps")
        } else {
            target_dir.join(target.as_str()).join(clib.profile)
        }
        .join(&clib_name);
        if !clib_path.exists() {
            eprintln!(
                "The clib for {target} does not exist at {clib_path}!",
                clib_path = clib_path.display(),
                target = target.as_str(),
            );
        }
        let mut dst = File::create(clibs_dir.join(format!(
            "{target}.{clib_name}",
            target = target.simplified_target_triple()
        )))?;
        if compress {
            let encoder = zstd::Encoder::new(dst, clib.compression_level)?;
            io::copy(&mut File::open(clib_path)?, &mut encoder.auto_finish())?;
        } else {
            io::copy(&mut File::open(clib_path)?, &mut dst)?;
        }
    }

    Ok(())
}
