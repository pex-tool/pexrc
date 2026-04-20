// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use clap::Args;
use pex::Pex;

#[derive(Args)]
pub(crate) struct ExtractArgs {
    /// The path to extract distribution as wheels to.
    #[arg(short = 'f', long, visible_aliases = ["find-links", "repo"])]
    dest_dir: PathBuf,

    /// Also extract a wheel for the PEX file sources.
    #[arg(short = 'D', long, default_value_t = false)]
    sources: bool,

    /// Use the current system time to generate timestamps for the extracted distributions.
    #[arg(
        long,
        default_value_t = false,
        long_help = "\
Use the current system time to generate timestamps for the extracted distributions. Otherwise, Pex
will use midnight on January 1, 1980. By using system time, the extracted distributions will not be
reproducible, meaning that if you were to re-run extraction against the same PEX file then the
newly extracted distributions would not be byte-for-byte identical distributions extracted in prior
runs."
    )]
    use_system_time: bool,

    /// Serve the `--find-links` repo.
    #[arg(long, default_value_t = false)]
    serve: bool,

    /// The port to serve the --find-links repo on.
    #[arg(long)]
    port: Option<u16>,

    /// The path of a file to write the `<pid>:<port>` of the find links server to.
    #[arg(long)]
    pid_file: Option<PathBuf>,
}

pub(crate) fn extract(_python: &Path, _pex: Pex, _args: ExtractArgs) -> anyhow::Result<()> {
    todo!("`PEX_TOOLS=1 repository extract` is under development.")
}
