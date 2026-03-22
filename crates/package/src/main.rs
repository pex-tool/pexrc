// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs};

use anyhow::{anyhow, bail};
use clap::{Parser, ValueEnum};
use owo_colors::OwoColorize;
use pexrc_build_system::{classify_targets, ensure_tools_installed};

#[derive(Clone, Eq, PartialEq, ValueEnum)]
#[clap(rename_all = "kebab_case")]
enum Target {
    All,
    Current,
}

/// Pexrc Packaging System.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    #[arg(long)]
    profile: Option<String>,

    #[arg(long)]
    target: Option<Target>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbosity.into())
        .init();
    cli.color.write_global();

    let cargo: PathBuf = env!("CARGO").into();
    let process = Command::new(&cargo)
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = process.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "Failed to determine path to workspace Cargo.toml; process exited with {status:?}:\n\
            {stderr}",
            status = output.status,
            stderr = String::from_utf8_lossy(&output.stderr)
        );
    }
    let cargo_manifest_path: PathBuf = String::from_utf8(output.stdout)?.trim_end().into();
    let cargo_manifest_contents = fs::read_to_string(&cargo_manifest_path)?;
    let cargo_manifest_dir = cargo_manifest_path.parent().ok_or_else(|| {
        anyhow!(
            "Failed to determine cargo project root directory from workspace Cargo.toml path: \
            {path}",
            path = cargo_manifest_path.display()
        )
    })?;
    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        cargo_manifest_dir.join("target")
    };
    let (_, glibc, found_tools) =
        ensure_tools_installed(&cargo, &cargo_manifest_contents, &target_dir, false)?;

    let rust_toolchain_contents = fs::read_to_string(cargo_manifest_dir.join("rust-toolchain"))?;
    let classified_targets = classify_targets(&rust_toolchain_contents, &glibc)?;

    let profile = cli.profile.as_deref().unwrap_or("dev");

    if cli.target.unwrap_or(Target::Current) == Target::Current {
        let result = Command::new(&cargo)
            .args(["build", "--profile", profile])
            .spawn()?
            .wait()?;
        if !result.success() {
            bail!("Build via cargo build failed!");
        }
    } else {
        let zigbuild_targets = classified_targets.iter_zigbuild_targets();
        if zigbuild_targets.len() > 0 {
            let mut command = Command::new(&cargo);
            command.args(["zigbuild", "--profile", profile]);
            for target in zigbuild_targets {
                command.args(["--target", target]);
            }
            command.env("PEXRC_TARGETS", "all");
            for found_tool in &found_tools {
                command.env(found_tool.env_var, &found_tool.path);
            }
            let result = command.spawn()?.wait()?;
            if !result.success() {
                bail!("Cross-build via cargo-zigbuild failed!");
            }
        }

        let xwin_targets = classified_targets.iter_xwin_targets();
        if xwin_targets.len() > 0 {
            let mut command = Command::new(&cargo);
            command.args(["xwin", "build", "--profile", profile]);
            for target in xwin_targets {
                command.args(["--target", target]);
            }
            command.env("PEXRC_TARGETS", "all");
            for found_tool in &found_tools {
                command.env(found_tool.env_var, &found_tool.path);
            }
            let result = command.spawn()?.wait()?;
            if !result.success() {
                bail!("Cross-build via cargo-xwin failed!");
            }
        }
    }

    anstream::println!("{}", "Build complete!".green());
    Ok(())
}
