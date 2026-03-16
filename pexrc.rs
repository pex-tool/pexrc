// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};
use pexrc::clibs::CLIB_BY_TARGET;
use pexrc::commands::{info, inject};

/// Pex Runtime Control.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inject a traditional PEX with the pexrc runtime.
    Inject {
        #[arg(long)]
        compression_level: Option<i64>,

        #[arg(long = "target")]
        #[arg(action=ArgAction::Append)]
        #[arg(value_parser=clap::builder::PossibleValuesParser::new(CLIB_BY_TARGET.keys()))]
        targets: Vec<String>,

        #[arg(value_name = "FILE")]
        pex: PathBuf,
    },
    /// Provide information about the supported target runtimes.
    Info,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbosity.into())
        .init();
    cli.color.write_global();

    match cli.command {
        Commands::Inject {
            pex,
            compression_level,
            targets,
        } => {
            let clibs = if !targets.is_empty() {
                Some(
                    targets
                        .into_iter()
                        .map(|target| {
                            CLIB_BY_TARGET.get(target.as_str()).copied().expect(
                                "The allowed --target values are all keys in the CLIB_BY_TARGET \
                                map.",
                            )
                        })
                        .collect::<HashSet<_>>(),
                )
            } else {
                None
            };
            inject::inject(&pex, compression_level, clibs)
        }
        Commands::Info => info::display(),
    }
}
