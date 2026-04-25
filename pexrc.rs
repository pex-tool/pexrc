// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::collections::HashSet;
use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use pex::Pex;
use pexrc::commands::{extract, info, inject, script};
use pexrc::embeds::{CLIB_BY_TARGET, PROXY_BY_TARGET};
use pexrc::source;
use target::Target;

/// Pex Runtime Control.
#[derive(Parser)]
#[command(version, about, long_about = None, styles = cli::STYLES)]
struct Cli {
    #[command(flatten)]
    verbosity: Option<clap_verbosity_flag::Verbosity>,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum CompressionMethod {
    Deflated,
    Zstd,
}

impl From<CompressionMethod> for zip::CompressionMethod {
    fn from(val: CompressionMethod) -> Self {
        match val {
            CompressionMethod::Deflated => zip::CompressionMethod::Deflated,
            CompressionMethod::Zstd => zip::CompressionMethod::Zstd,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Extract Pex dependencies as zstd wheels.
    Extract {
        #[arg(short = 'Z', long, value_enum, default_value_t = CompressionMethod::Zstd)]
        compression_method: CompressionMethod,

        #[arg(long)]
        compression_level: Option<i64>,

        /// The directory to extract the wheels to.
        #[arg(short = 'd', long)]
        dest_dir: PathBuf,

        /// The Pex to extract dependency wheels from. Can be a path or URL.
        #[arg(value_name = "PEX")]
        pex: String,
    },
    /// Inject a traditional PEX with the pexrc runtime.
    Inject {
        #[arg(short = 'Z', long, value_enum, default_value_t = CompressionMethod::Zstd)]
        compression_method: CompressionMethod,

        #[arg(long)]
        compression_level: Option<i64>,

        #[arg(long = "target")]
        #[arg(action=ArgAction::Append)]
        #[arg(value_parser=clap::builder::PossibleValuesParser::new(CLIB_BY_TARGET.keys()))]
        targets: Vec<String>,

        #[arg(value_name = "PEX", required = true)]
        pexes: Vec<String>,
    },
    /// Provide information about the supported target runtimes.
    Info,
    /// Create a Windows-style Python venv console script executable.
    Script {
        #[arg(long)]
        #[arg(value_parser=clap::builder::PossibleValuesParser::new(PROXY_BY_TARGET.keys()))]
        target: Option<String>,

        #[arg(short = 'p', long, required = true)]
        python: PathBuf,

        #[arg(short = 'o', long)]
        output_file: PathBuf,

        #[arg(value_name = "SCRIPT")]
        script: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging::init(cli.verbosity.map(|verbosity| verbosity.log_level_filter()))?;
    cli.color.write_global();

    match cli.command {
        Commands::Extract {
            compression_method,
            compression_level,
            dest_dir,
            pex,
        } => {
            let pex = source::to_path(pex, Some(&dest_dir))?;
            extract::to_dir(
                &dest_dir,
                Pex::load(&pex)?,
                compression_method.into(),
                compression_level,
            )
        }
        Commands::Inject {
            compression_method,
            compression_level,
            targets,
            pexes,
        } => {
            let (clibs, proxies) = if !targets.is_empty() {
                (
                    Some(
                        targets
                            .iter()
                            .map(|target| {
                                CLIB_BY_TARGET.get(target.as_str()).copied().expect(
                                "The allowed --target values are all keys in the CLIB_BY_TARGET \
                                map.",
                            )
                            })
                            .collect::<HashSet<_>>(),
                    ),
                    Some(
                        targets
                            .iter()
                            .map(|target| {
                                PROXY_BY_TARGET.get(target.as_str()).copied().expect(
                                "The allowed --target values are all keys in the PROXY_BY_TARGET \
                                map.",
                            )
                            })
                            .collect::<HashSet<_>>(),
                    ),
                )
            } else {
                (None, None)
            };
            let pexes = pexes
                .into_iter()
                .map(|source| source::to_path(source, None))
                .collect::<anyhow::Result<Vec<_>>>()?;
            inject::inject_all(
                pexes,
                compression_method.into(),
                compression_level,
                clibs.as_ref(),
                proxies.as_ref(),
            )
        }
        Commands::Info => info::display(),
        Commands::Script {
            target,
            python,
            script,
            output_file,
        } => {
            if let Some(target) = target {
                script::create(target.as_str(), &python, &script, &output_file)
            } else {
                let current_target = Target::current()?;
                script::create(
                    current_target.simplified_target_triple().as_ref(),
                    &python,
                    &script,
                    &output_file,
                )
            }
        }
    }
}
