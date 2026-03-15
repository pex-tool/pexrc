// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use anyhow::{anyhow, bail};
use cache::CacheDir;
use const_format::str_split;
use interpreter::{InterpreterConstraints, SearchPath, SelectionStrategy};
use pex::{Pex, PexPath};
use pexrs::venv_dir;
use platform::path_as_str;

const SH_BOOT_SHEBANG: &[u8] = b"#!/bin/sh\n";
const PARTS: [&str; 3] = str_split!(include_str!("boot.sh"), "# --- vars --- #\n");

pub fn shebang(pex: &Path) -> anyhow::Result<Option<String>> {
    let mut sh_boot_shebang_buffer: [_; SH_BOOT_SHEBANG.len()] = [0; SH_BOOT_SHEBANG.len()];
    let mut pex_fp = File::open(pex)?;
    match pex_fp.read_exact(&mut sh_boot_shebang_buffer) {
        Ok(()) => {
            if sh_boot_shebang_buffer != SH_BOOT_SHEBANG {
                return Ok(None);
            }
        }
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => bail!(
            "Failed to determine if {pex} uses a `--sh-boot` shebang header: {err}",
            pex = pex.display()
        ),
    };

    let pex = Pex::load(pex)?;
    let pex_info = pex.info();
    let pex_path = PexPath::from_pex_info(pex_info, false);
    let additional_pexes = pex_path.load_pexes()?;

    let venv_dir = venv_dir(None, &pex, &SearchPath::EMPTY, &additional_pexes)?;
    let venv_relpath = venv_dir.strip_prefix(CacheDir::root()?)?;

    let interpreter_constraints =
        InterpreterConstraints::try_from(&pex_info.interpreter_constraints)?;
    let selection_strategy: SelectionStrategy = pex_info.interpreter_selection_strategy.into();
    let pythons = interpreter_constraints
        .calculate_compatible_binary_names(selection_strategy)
        .into_iter()
        .map(|binary_name| {
            binary_name
                .into_string()
                .map_err(|err| anyhow!("{err}", err = err.display()))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(Some(format!(
        "{header}{vars}{body}\n",
        header = PARTS[0],
        vars = PARTS[1]
            .replace(
                "{pexrc_root}",
                pex_info.pex_root.as_deref().unwrap_or_default()
            )
            .replace("{venv_relpath}", path_as_str(venv_relpath)?)
            .replace("{pythons}", &pythons.join("\n")),
        body = PARTS[2]
    )))
}
