// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::{env, io};

use anyhow::{Context, anyhow, bail};
use include_dir::{Dir, include_dir};
use itertools::Itertools;
use log::info;
use logging_timer::time;
use pexrs::{Algorithm, boot};
use tempfile::NamedTempFile;
use which::which;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

static CLIBS_DIR: Dir<'_> = include_dir!("$CLIBS_DIR");

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let algorithm = env::args_os()
        .find_map(|arg| {
            if arg == "--init" {
                Some(Algorithm::TryForEachInit)
            } else {
                None
            }
        })
        .unwrap_or(Algorithm::TryForEach);

    let gc = !env::args_os().any(|arg| arg == "--keep");

    let compression_level = if let Some(raw_value) = env::args_os()
        .tuple_windows::<(_, _)>()
        .find_map(|(flag, value)| {
            if flag.as_os_str() == "--compression" {
                Some(value)
            } else {
                None
            }
        }) {
        let value = raw_value.to_str().ok_or_else(|| {
            anyhow!("The --compression given is not parseable as an integer: {raw_value:?}")
        })?;
        value.parse::<i64>()?
    } else {
        3
    };

    info!("Embedded clibs:");
    for (idx, file) in CLIBS_DIR.files().enumerate() {
        info!(
            "{idx} {clib} {size}",
            idx = idx + 1,
            clib = file.path().display(),
            size = file.contents().len()
        )
    }

    if let Some(pex_file) = env::args_os().nth(1).as_deref() {
        let pex_path = Path::new(pex_file);
        let python = which("python").with_context(|| {
            format!(
                "Failed to find a Python executable to boot {pex} with.",
                pex = pex_path.display()
            )
        })?;

        info!(
            "Using compression level {compression_level}, algorithm {algorithm} and {modifier} gc \
            the extraction dir.",
            modifier = if gc { "will" } else { "will not" }
        );
        transcode(pex_path, Some(compression_level))?;

        info!("Booting PEX with {python}.", python = python.display());
        boot(python, pex_path, Some(algorithm), gc)
    } else {
        bail!(
            "Usage: {} [pex file]",
            env::args().next().unwrap_or_else(|| "pexrc".to_string())
        )
    }
}

#[time("debug", "{}")]
fn transcode(zip_path: &Path, compression_level: Option<i64>) -> anyhow::Result<()> {
    let zip_read_fp = File::open(zip_path)?;

    let mut src_zip = ZipArchive::new(&zip_read_fp)?;
    let prefix = {
        let first_entry = src_zip.by_index(0)?;
        let zip_start = first_entry.header_start();
        if zip_start > 0 {
            let mut prefix_reader = File::open(zip_path)?.take(zip_start);
            let mut prefix = Vec::with_capacity(zip_start.try_into().with_context(|| {
                format!(
                    "The zip prefix is {zip_start} bytes which is bigger than the system pointer \
                    size of {ptr_size} bits.",
                    ptr_size = usize::BITS
                )
            })?);
            prefix_reader.read_to_end(&mut prefix)?;
            Some(prefix)
        } else {
            None
        }
    };

    let mut dst_zip_fp = if let Some(parent_dir) = zip_path.parent() {
        NamedTempFile::new_in(parent_dir)?
    } else {
        NamedTempFile::new()?
    };
    if let Some(prefix) = prefix {
        dst_zip_fp.write_all(&prefix)?;
    }
    let mut dst_zip = ZipWriter::new(&dst_zip_fp);

    let file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Zstd)
        .compression_level(compression_level);
    let directory_options = SimpleFileOptions::default();
    for index in 0..src_zip.len() {
        let mut src_file = src_zip.by_index(index)?;
        if src_file.is_dir() {
            dst_zip.add_directory(src_file.name(), directory_options)?
        } else {
            dst_zip.start_file(src_file.name(), file_options)?;
            io::copy(&mut src_file, &mut dst_zip)?;
        }
    }
    dst_zip.finish()?;
    dst_zip_fp.persist(zip_path)?;
    Ok(())
}
