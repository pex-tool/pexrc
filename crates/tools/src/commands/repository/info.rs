// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;
use pex::{Pex, PexPath};
use serde_json::json;

use crate::json;
use crate::output::Output;
use crate::resolve::resolve;

#[derive(Args)]
pub(crate) struct InfoArgs {
    /// Print the distributions requirements in addition to its name version and path.
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    /// Pretty-print verbose output json with the given indent.
    #[arg(short = 'i', long)]
    indent: Option<u8>,

    /// A file to output the distribution information to; STDOUT by default.
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

pub(crate) fn display(python: &Path, pex: Pex, args: InfoArgs) -> anyhow::Result<()> {
    let pex_path = PexPath::from_pex_info(&pex.info, true);
    let additional_pexes = pex_path.load_pexes()?;
    let (_, wheels) = resolve(python, &pex, &additional_pexes)?;

    let mut output = Output::new(args.output.as_deref())?;
    for (project_name, wheel_info) in wheels {
        if args.verbose {
            json::serialize(
                &mut output,
                &json!({
                    "project_name": project_name,
                    "version": wheel_info.version,
                    "requires_python": wheel_info.requires_python,
                    "requires_dists": wheel_info.requires_dists,
                    "location": pex.path.join(".deps").join(wheel_info.file_name)
                }),
                args.indent,
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
