// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(const_convert)]
#![feature(const_trait_impl)]

mod commands;
mod json;
mod output;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use logging_timer::time;
use pex::Pex;

use crate::commands::venv::VenvArgs;
use crate::commands::{extract, info, venv};

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
    Interpreter {
        /// Print all compatible interpreters, preferred first.
        #[arg(short = 'a', long, default_value_t = false)]
        all: bool,

        /// Provide more information about the interpreter in JSON format.
        #[arg(short = 'v', long, action = clap::ArgAction::Count, long_help = "\
Provide more information about the interpreter in JSON format.
Once: include the interpreter requirement and platform in addition to its path.
Twice: include the interpreter's supported tags.
Thrice: include the interpreter's environment markers and its venv affiliation, if any.
"
        )]
        verbose: u8,

        /// Pretty-print verbose output JSON with the given indent.
        #[arg(short = 'i', long)]
        indent: Option<u8>,

        /// A file to output the Python interpreter path to; STDOUT by default.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Generates a dot graph of the dependencies contained in a PEX.
    Graph,
    /// Interact with the Python distribution repository contained in a PEX.
    Repository,
    /// Creates a venv from the PEX.
    Venv(VenvArgs),
}

impl AsRef<str> for Commands {
    fn as_ref(&self) -> &str {
        match self {
            Commands::Extract { .. } => "extract",
            Commands::Graph => "graph",
            Commands::Info { .. } => "info",
            Commands::Interpreter { .. } => "interpreter",
            Commands::Repository => "repository",
            Commands::Venv { .. } => "venv",
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
pub fn main(python: &Path, pex: &Path, argv: Vec<String>) -> anyhow::Result<()> {
    let cli = parse_cli(pex, argv)?;
    match cli.command {
        Commands::Extract { dest_dir } => extract::unzip(pex, &dest_dir),
        Commands::Info { indent, output } => {
            info::display(Pex::load(pex)?, indent, output.as_deref())
        }
        Commands::Interpreter {
            all,
            verbose,
            indent,
            output,
        } => commands::interpreter::display(
            python,
            Pex::load(pex)?,
            all,
            verbose,
            indent,
            output.as_deref(),
        ),
        Commands::Venv(venv_args) => venv::create(python, Pex::load(pex)?, venv_args),
        command => todo!(
            "`PEX_TOOLS=1 {pex} {command}` is under development.",
            pex = pex.display(),
            command = command.as_ref()
        ),
    }
}
