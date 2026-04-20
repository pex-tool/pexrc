// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Subcommand;
use pex::{Pex, PexPath};
use serde_json::json;

use crate::json;
use crate::output::Output;
use crate::resolve::resolve;

#[derive(Subcommand)]
pub(crate) enum Repository {
    /// Print information about the distributions in a PEX file.
    Info {
        /// Print the distributions requirements in addition to its name version and path.
        #[arg(short = 'v', long, default_value_t = false)]
        verbose: bool,

        /// Pretty-print verbose output json with the given indent.
        #[arg(short = 'i', long)]
        indent: Option<u8>,

        /// A file to output the distribution information to; STDOUT by default.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Extract all distributions from a PEX file.
    Extract,
}

pub(crate) fn info(
    python: &Path,
    pex: Pex,
    verbose: bool,
    indent: Option<u8>,
    output: Option<&Path>,
) -> anyhow::Result<()> {
    let pex_path = PexPath::from_pex_info(&pex.info, true);
    let additional_pexes = pex_path.load_pexes()?;
    let (_, wheels) = resolve(python, &pex, &additional_pexes)?;

    let mut output = Output::new(output)?;
    for (project_name, wheel_info) in wheels {
        if verbose {
            json::serialize(
                &mut output,
                &json!({
                    "project_name": project_name,
                    "version": wheel_info.version,
                    "requires_python": wheel_info.requires_python,
                    "requires_dists": wheel_info.requires_dists,
                    "location": pex.path.join(wheel_info.file_name)
                }),
                indent,
            )?;
        } else {
            writeln!(
                output,
                "{project_name} {version} {location}",
                version = wheel_info.version,
                location = pex.path.join(wheel_info.file_name).display()
            )?;
        }
    }

    Ok(())
}

pub(crate) fn extract() -> anyhow::Result<()> {
    todo!("`PEX_TOOLS=1 repository extract` is under development.")
}
