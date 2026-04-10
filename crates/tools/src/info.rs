// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::io::Write;
use std::path::Path;

use fs_err::File;
use logging_timer::time;
use pex::Pex;
use serde::Serialize;
use serde_json::ser::PrettyFormatter;

// N.B.: This supports a crazy API without allocating. Perhaps just write our own PrettyFormatter
// that accepts a number for indent space count instead of a byte slice.
const INDENT_BUFFER: &[u8] = &[b' '; usize::from(u8::MAX)];

fn indent_serializer<W: Write>(
    indent: u8,
    out: W,
) -> serde_json::Serializer<W, PrettyFormatter<'static>> {
    serde_json::Serializer::with_formatter(
        out,
        PrettyFormatter::with_indent(&INDENT_BUFFER[0..usize::from(indent)]),
    )
}

#[time("debug", "{}")]
pub(crate) fn display(pex: Pex, indent: Option<u8>, output: Option<&Path>) -> anyhow::Result<()> {
    match (indent, output) {
        (Some(indent), Some(path)) => {
            let mut serializer = indent_serializer(indent, File::create(path)?);
            pex.info.serialize(&mut serializer)?;
        }
        (Some(indent), None) => {
            let mut serializer = indent_serializer(indent, io::stdout());
            pex.info.serialize(&mut serializer)?;
        }
        (None, Some(path)) => {
            serde_json::to_writer(File::create(path)?, &pex.info)?;
        }
        (None, None) => {
            serde_json::to_writer(io::stdout(), &pex.info)?;
        }
    }
    Ok(())
}
