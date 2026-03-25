// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::string::ToString;
use std::sync::LazyLock;
use std::{env, fs};

use anyhow::{anyhow, bail};
use clap::builder::Str;
use clap::{ArgAction, Parser};
use owo_colors::OwoColorize;
use pexrc_build_system::{all_targets, classify_targets, ensure_tools_installed};

static CARGO: LazyLock<PathBuf> = LazyLock::new(|| env!("CARGO").into());

static CARGO_MANIFEST_PATH: LazyLock<anyhow::Result<PathBuf>> = LazyLock::new(|| {
    let process = Command::new(CARGO.as_path())
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
    Ok(String::from_utf8(output.stdout)?.trim_end().into())
});

static CARGO_MANIFEST_DIR: LazyLock<anyhow::Result<&Path>> =
    LazyLock::new(|| match CARGO_MANIFEST_PATH.as_ref() {
        Ok(cargo_manifest_path) => cargo_manifest_path.parent().ok_or_else(|| {
            anyhow!(
                "Failed to determine cargo project root directory from workspace Cargo.toml \
                    path: {path}",
                path = cargo_manifest_path.display()
            )
        }),
        Err(err) => bail!("Failed to determine cargo workspace root dir: {err}"),
    });

static ALL_TARGETS: LazyLock<String> = LazyLock::new(|| "all".to_string());

static AVAILABLE_TARGETS: LazyLock<Vec<Str>> = LazyLock::new(|| {
    let mut available_targets = vec![Str::from(ALL_TARGETS.as_str())];
    let cargo_manifest_dir = match CARGO_MANIFEST_DIR.as_ref() {
        Ok(manifest_dir) => manifest_dir,
        Err(err) => panic!("Failed to determine cargo workspace root dir: {err}"),
    };
    let rust_toolchain_contents =
        match fs::read_to_string(cargo_manifest_dir.join("rust-toolchain")) {
            Ok(contents) => contents,
            Err(err) => panic!("Failed to read rust-toolchain file: {err}"),
        };
    match all_targets(&rust_toolchain_contents) {
        Ok(targets) => {
            for target in targets {
                available_targets.push(Str::from(target))
            }
        }
        Err(err) => panic!("Failed to parse rust-toolchain file: {err}"),
    }
    available_targets
});

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

    #[arg(long = "target")]
    #[arg(action=ArgAction::Append)]
    #[arg(value_parser=clap::builder::PossibleValuesParser::new(AVAILABLE_TARGETS.iter()))]
    targets: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbosity.into())
        .init();
    cli.color.write_global();

    let cargo = CARGO.as_path();
    let cargo_manifest_path = match CARGO_MANIFEST_PATH.as_ref() {
        Ok(manifest_path) => manifest_path,
        Err(err) => bail!("{err}"),
    };
    let cargo_manifest_contents = fs::read_to_string(cargo_manifest_path)?;
    let cargo_manifest_dir = match CARGO_MANIFEST_DIR.as_ref() {
        Ok(manifest_dir) => manifest_dir,
        Err(err) => bail!("{err}"),
    };
    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        cargo_manifest_dir.join("target")
    };
    let (_, glibc, found_tools) =
        ensure_tools_installed(cargo, &cargo_manifest_contents, &target_dir, false)?;

    let rust_toolchain_contents = fs::read_to_string(cargo_manifest_dir.join("rust-toolchain"))?;
    let classified_targets = classify_targets(&rust_toolchain_contents, &glibc)?;

    let profile = cli.profile.as_deref().unwrap_or("dev");

    if cli.targets.is_empty() {
        let result = Command::new(cargo)
            .args(["build", "--profile", profile])
            .spawn()?
            .wait()?;
        if !result.success() {
            bail!("Build via cargo build failed!");
        }
    } else {
        let targeted: HashSet<String> = if cli.targets.contains(&ALL_TARGETS) {
            AVAILABLE_TARGETS.iter().map(Str::to_string).collect()
        } else {
            cli.targets.into_iter().collect()
        };
        let zigbuild_targets = classified_targets
            .iter_zigbuild_targets()
            .filter(|target| {
                // Strip the `.{glibc-version}` suffix from `*-gnu.{glibc-version}` targets.
                // TODO: Encode classified targets such that we don't need to use string parsing
                //  here to undo earlier string concatenation of the glibc-version when classifying
                //  the targets.
                let target = if target.contains("-gnu.")
                    && let Some(target) = target.splitn(2, ".").take(1).next()
                {
                    target
                } else {
                    target
                };
                targeted.contains(target)
            })
            .collect::<Vec<_>>();
        if !zigbuild_targets.is_empty() {
            let mut command = Command::new(cargo);
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

        let xwin_targets = classified_targets
            .iter_xwin_targets()
            .filter(|target| targeted.contains(*target))
            .collect::<Vec<_>>();
        if !xwin_targets.is_empty() {
            let mut command = Command::new(cargo);
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
