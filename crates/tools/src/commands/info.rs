// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Args;
use logging_timer::time;
use pex::Pex;

use crate::output::Output;

#[derive(Args)]
pub(crate) struct InfoArgs {
    /// Pretty-print PEX-INFO JSON with the given indent.
    #[arg(short = 'i', long)]
    indent: Option<u8>,

    /// A file to output the PEX-INFO JSON to; STDOUT by default.
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

#[time("debug", "{}")]
pub(crate) fn display(pex: Pex, args: InfoArgs) -> anyhow::Result<()> {
    crate::json::serialize(
        Output::new(args.output.as_deref())?,
        &pex.info.raw(),
        args.indent,
    )
}
