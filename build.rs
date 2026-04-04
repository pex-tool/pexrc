// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(exact_size_is_empty)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::{env, io};

use anyhow::{anyhow, bail};
use bstr::ByteSlice;
use build_system::{
    ClassifiedTargets,
    EmbedsConfiguration,
    FoundTool,
    Target,
    classify_targets,
    ensure_tools_installed,
};
use fs_err as fs;
use fs_err::File;
use itertools::Itertools;

const CARGO_UNSTABLE_FLAGS: [&str; 2] = [
    // N.B.: This gets us the stdlib compiled locally and LTO'ed into the binaries and libraries
    // we produce for space (and speed) savings.
    // See https://github.com/johnthagen/min-sized-rust for the inspiration.
    "-Zbuild-std=std,panic_abort",
    "-Zbuild-std-features=optimize_for_size",
];

static RUSTFLAGS: LazyLock<String> = LazyLock::new(|| {
    [
        // N.B.: These options help reduce binary size.
        // See https://github.com/johnthagen/min-sized-rust for the inspiration.
        "-Zunstable-options",
        "-Zfmt-debug=none",
        "-Zlocation-detail=none",
        "-Cpanic=immediate-abort",
        // This gets us lib musl dynamic linking.
        "-Ctarget-feature=-crt-static",
    ]
    .join(" ")
});

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

    let (mut embeds, glibc, found_tools) =
        ensure_tools_installed(&cargo, &cargo_manifest_contents, &target_dir, true)?;
    println!("cargo::rerun-if-env-changed=PROFILE");
    let profile = env::var("PROFILE")?;
    let embeds_configuration = embeds.configuration_for(&profile);

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

    let embeds_dir = {
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let embeds_dir = PathBuf::from(out_dir).join("embeds");
        if all_targets {
            embeds_dir.join("all")
        } else {
            embeds_dir.join("current")
        }
    };
    fs::create_dir_all(&embeds_dir)?;
    println!(
        "cargo::rustc-env=EMBEDS_DIR={embeds_dir}",
        embeds_dir = embeds_dir.to_str().ok_or_else(|| {
            anyhow!(
                "Build cannot proceed with project housed in a non-UTF-8 directory: {embeds_dir}",
                embeds_dir = embeds_dir.display()
            )
        })?
    );

    // N.B.: We need to use a custom --target-dir to avoid a deadlock on the ambient target that
    // would otherwise occur calling into cargo build recursively below.
    let tgt_path = target_dir.join("embeds");
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
            embeds_configuration.profile,
            &found_tools,
            targets.iter_zigbuild_targets().map(Target::zigbuild_target),
        )?;
        custom_cargo_build(
            &cargo,
            &["xwin", "build", "--target-dir", tgt_arg],
            embeds_configuration.profile,
            &found_tools,
            targets.iter_xwin_targets().map(Target::as_str),
        )?;
        collect_embeds(&targets, &tgt_path, embeds_configuration, &embeds_dir, true)
    } else {
        let target = env::var("TARGET")?;
        let targets = ClassifiedTargets::parse([target.as_str()].into_iter(), &glibc);
        custom_cargo_build(
            &cargo,
            &["build", "--target-dir", tgt_arg],
            embeds_configuration.profile,
            &found_tools,
            [Target::current(&glibc).as_str()].into_iter(),
        )?;
        collect_embeds(&targets, &tgt_path, embeds_configuration, &embeds_dir, true)
    }
}

fn custom_cargo_build<'a>(
    cargo: &Path,
    custom_build_args: &[&str],
    profile: &str,
    found_tools: &[FoundTool],
    targets: impl ExactSizeIterator<Item = &'a str>,
) -> anyhow::Result<()> {
    if targets.is_empty() {
        return Ok(());
    }

    let mut cmd = Command::new(cargo);
    let cmd = cmd
        .stderr(Stdio::piped())
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TERM_COLOR", "always")
        .env("RUSTFLAGS", RUSTFLAGS.as_str());
    for found_tool in found_tools {
        println!(
            "cargo::rustc-env={env_var}={path}",
            env_var = found_tool.env_var,
            path = found_tool.path.display()
        );
        cmd.env(found_tool.env_var, &found_tool.path);
    }
    cmd.args(custom_build_args)
        .args(CARGO_UNSTABLE_FLAGS)
        .args(["--package", "clib", "--package", "python-proxy"]);
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

fn collect_embeds<'a>(
    targets: &'a ClassifiedTargets<'a>,
    target_dir: &Path,
    embeds_configuration: &'a EmbedsConfiguration<'a>,
    embeds_dir: &Path,
    compress: bool,
) -> anyhow::Result<()> {
    let clibs_dir = embeds_dir.join("clibs");
    fs::create_dir_all(&clibs_dir)?;
    let proxies_dir = embeds_dir.join("proxies");
    fs::create_dir_all(&proxies_dir)?;
    for target in targets.iter_all_targets() {
        let clib_name = target.shared_library_name("pexrc");
        collect_embed(
            &clib_name,
            &clibs_dir,
            embeds_configuration,
            target,
            target_dir,
            compress,
        )?;
        let python_proxy_name = target.binary_name("python-proxy", None);
        collect_embed(
            &python_proxy_name,
            &proxies_dir,
            embeds_configuration,
            target,
            target_dir,
            compress,
        )?;
    }
    Ok(())
}

fn collect_embed<'a>(
    embed_name: &str,
    embed_dir: &Path,
    embeds_configuration: &'a EmbedsConfiguration<'a>,
    target: &'a Target,
    target_dir: &Path,
    compress: bool,
) -> anyhow::Result<()> {
    let embed_path = target_dir
        .join(target.as_str())
        .join(embeds_configuration.profile)
        .join(embed_name);
    if !embed_path.exists() {
        eprintln!(
            "The embed for {target} does not exist at {embed_path}!",
            embed_path = embed_path.display(),
            target = target.as_str(),
        );
    }
    let mut dst = File::create(embed_dir.join(format!(
        "{target}.{embed_name}",
        target = target.simplified_target_triple()
    )))?;
    if compress {
        let encoder = zstd::Encoder::new(dst, embeds_configuration.compression_level)?;
        io::copy(&mut File::open(embed_path)?, &mut encoder.auto_finish())?;
    } else {
        io::copy(&mut File::open(embed_path)?, &mut dst)?;
    }
    Ok(())
}
