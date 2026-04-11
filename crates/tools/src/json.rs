// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;

use serde::Serialize;
use serde_json::ser::PrettyFormatter;

// N.B.: This supports a crazy API without allocating. Perhaps just write our own PrettyFormatter
// that accepts a number for indent space count instead of a byte slice.
const INDENT_BUFFER: &[u8] = &[b' '; usize::from(u8::MAX)];

pub(crate) fn serialize(
    mut out: impl Write,
    value: &impl Serialize,
    indent: Option<u8>,
) -> anyhow::Result<()> {
    if let Some(indent) = indent {
        let mut serializer = serde_json::Serializer::with_formatter(
            &mut out,
            PrettyFormatter::with_indent(&INDENT_BUFFER[0..usize::from(indent)]),
        );
        value.serialize(&mut serializer)?;
    } else {
        serde_json::to_writer(&mut out, value)?;
    }
    writeln!(&mut out)?;
    Ok(())
}
