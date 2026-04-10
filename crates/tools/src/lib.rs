// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(const_convert)]
#![feature(const_trait_impl)]

mod extract;
mod info;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use logging_timer::time;
use pex::Pex;

/// Pex Tools.
#[derive(Parser)]
#[command(version, about, long_about = None, bin_name = "PEX_TOOLS=1 {PEX}", styles = cli::STYLES)]
struct Cli {
    #[command(flatten)]
    verbosity: Option<clap_verbosity_flag::Verbosity>,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract the PEX to a directory.
    Extract {
        /// The directory to extract the PEX into.
        #[arg()]
        dest_dir: PathBuf,
    },
    /// Dumps the PEX-INFO JSON contained in a PEX.
    Info {
        /// Pretty-print PEX-INFO JSON with the given indent.
        #[arg(short = 'i', long)]
        indent: Option<u8>,

        /// A file to output the PEX-INFO JSON to; STDOUT by default.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Prints the path of the preferred interpreter to run the given PEX with, if any.
    Interpreter,
    /// Generates a dot graph of the dependencies contained in a PEX.
    Graph,
    /// Interact with the Python distribution repository contained in a PEX.
    Repository,
    /// Creates a venv from the PEX.
    Venv,
}

impl AsRef<str> for Commands {
    fn as_ref(&self) -> &str {
        match self {
            Commands::Extract { .. } => "extract",
            Commands::Info { .. } => "info",
            Commands::Interpreter => "interpreter",
            Commands::Graph => "graph",
            Commands::Repository => "repository",
            Commands::Venv => "venv",
        }
    }
}

#[time("debug", "{}")]
fn parse_cli(pex: &Path, argv: Vec<String>) -> anyhow::Result<Cli> {
    let cli = Cli::parse_from(
        [pex.to_string_lossy().into_owned()]
            .iter()
            .chain(argv.iter()),
    );
    logging::init(cli.verbosity.map(|verbosity| verbosity.log_level_filter()))?;
    cli.color.write_global();
    Ok(cli)
}

#[time("debug", "{}")]
pub fn main(pex: &Path, argv: Vec<String>) -> anyhow::Result<()> {
    let cli = parse_cli(pex, argv)?;
    match cli.command {
        Commands::Extract { dest_dir } => extract::unzip(pex, &dest_dir),
        Commands::Info { indent, output } => {
            info::display(Pex::load(pex)?, indent, output.as_deref())
        }
        command => todo!(
            "`PEX_TOOLS=1 {pex} {command}` is under development.",
            pex = pex.display(),
            command = command.as_ref()
        ),
    }
}
