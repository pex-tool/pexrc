// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::io::{ErrorKind, Read, Seek, Write};
use std::path::Path;

use anyhow::{anyhow, bail};
use cache::CacheDir;
use const_format::str_split;
use fs_err as fs;
use fs_err::File;
use interpreter::{InterpreterConstraints, SearchPath, SelectionStrategy};
use pex::{Pex, PexPath};
use pexrs::venv_dir;
use platform::path_as_str;
use zip::ZipWriter;
use zip::write::{FileOptionExtension, FileOptions};

const SH_BOOT_SHEBANG: &[u8] = b"#!/bin/sh\n";
const SH_BOOT_PARTS: [&str; 4] = str_split!(include_str!("boot.sh"), "# --- split --- #\n");

pub fn sh_boot_shebang(pex: &Path, escaped: bool) -> anyhow::Result<Option<String>> {
    let pex = Pex::load(pex)?;

    let mut sh_boot_shebang_buffer: [_; SH_BOOT_SHEBANG.len()] = [0; SH_BOOT_SHEBANG.len()];
    let mut pex_fp = File::open(pex.file())?;
    match pex_fp.read_exact(&mut sh_boot_shebang_buffer) {
        Ok(()) => {
            if sh_boot_shebang_buffer != SH_BOOT_SHEBANG {
                return Ok(None);
            }
        }
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => bail!(
            "Failed to determine if {pex} uses a `--sh-boot` shebang header: {err}",
            pex = pex.path.display()
        ),
    };
    let pex_path = PexPath::from_pex_info(&pex.info, false);
    let additional_pexes = pex_path.load_pexes()?;

    let venv_dir = venv_dir(None, &pex, &SearchPath::EMPTY, &additional_pexes)?;
    let venv_relpath = venv_dir.strip_prefix(CacheDir::root()?)?;

    let interpreter_constraints =
        InterpreterConstraints::try_from(&pex.info.interpreter_constraints)?;
    let selection_strategy: SelectionStrategy = pex.info.interpreter_selection_strategy.into();
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
        "{shebang}{start_escape}{header}{vars}{body}{end_escape}\n",
        shebang = SH_BOOT_PARTS[0], // N.B.: SH_BOOT_SHEBANG
        start_escape = if escaped { "'''': pshprs\n" } else { "" },
        header = SH_BOOT_PARTS[1],
        vars = SH_BOOT_PARTS[2]
            .replace(
                "{pexrc_root}",
                pex.info.pex_root.as_deref().unwrap_or_default(),
            )
            .replace("{venv_relpath}", path_as_str(venv_relpath)?)
            .replace("{pythons}", &pythons.join("\n")),
        body = SH_BOOT_PARTS[3].trim_end(),
        end_escape = if escaped { "\n'''\n" } else { "\n" },
    )))
}

const PY_BOOT: &[u8] = include_bytes!("boot.py");

pub fn inject_boot<T: FileOptionExtension + Copy>(
    zip: &mut ZipWriter<impl Write + Seek>,
    file_options: FileOptions<T>,
) -> anyhow::Result<()> {
    zip.start_file("__pex__/__init__.py", file_options)?;
    zip.write_all(PY_BOOT)?;
    zip.start_file("__main__.py", file_options)?;
    zip.write_all(PY_BOOT)?;
    Ok(())
}

pub fn write_boot(dest_dir: &Path, shebang: &str) -> anyhow::Result<()> {
    let main_py_path = dest_dir.join("__main__.py");
    let mut file = File::create_new(&main_py_path)?;
    file.write_all(shebang.as_bytes().trim_ascii_end())?;
    file.write_all(b"\n\n")?;
    file.write_all(PY_BOOT)?;
    fs::copy(&main_py_path, dest_dir.join("__pex__").join("__init__.py"))?;
    platform::mark_executable(file.file_mut())?;
    platform::symlink_or_link_or_copy(&main_py_path, dest_dir.join("pex"), true)?;
    Ok(())
}
