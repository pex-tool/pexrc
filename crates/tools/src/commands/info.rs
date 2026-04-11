// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use logging_timer::time;
use pex::Pex;

use crate::output::Output;

#[time("debug", "{}")]
pub(crate) fn display(pex: Pex, indent: Option<u8>, output: Option<&Path>) -> anyhow::Result<()> {
    crate::json::serialize(Output::new(output)?, &pex.info, indent)
}
