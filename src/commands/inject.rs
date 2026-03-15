// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::Context;
use log::info;
use owo_colors::OwoColorize;
use platform::mark_executable;
use python::{Resources, embedded};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::clibs::CLIBS_DIR;
use crate::sh_boot;

const MAIN: &[u8] = include_bytes!("../../python/pexrc/__init__.py");

pub fn inject(
    pex: &Path,
    compression_level: Option<i64>,
    clibs: Option<HashSet<&Path>>,
) -> anyhow::Result<()> {
    let zip_read_fp = File::open(pex)?;
    let mut src_zip = ZipArchive::new(&zip_read_fp)?;
    let prefix = if let Some(sh_boot_shebang) = sh_boot::shebang(pex)? {
        Some(sh_boot_shebang.into_bytes())
    } else {
        let first_entry = src_zip.by_index(0)?;
        let zip_start = first_entry.header_start();
        if zip_start > 0 {
            let mut prefix_reader = File::open(pex)?.take(zip_start);
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

    let mut dst_zip_fp = if let Some(parent_dir) = pex.parent() {
        NamedTempFile::new_in(parent_dir)?
    } else {
        NamedTempFile::new()?
    };
    if let Some(prefix) = prefix {
        dst_zip_fp.write_all(&prefix)?;
    }
    let mut dst_zip = ZipWriter::new(&dst_zip_fp);

    let zstd_file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Zstd)
        .compression_level(compression_level);
    let other_file_options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let directory_options = SimpleFileOptions::default();
    for index in 0..src_zip.len() {
        let mut src_file = src_zip.by_index(index)?;
        let entry_name = src_file.name();
        if [".bootstrap/", "__pex__/"]
            .into_iter()
            .any(|prefix| entry_name.starts_with(prefix))
            || entry_name == "__main__.py"
        {
            continue;
        }
        if src_file.is_dir() {
            dst_zip.add_directory(entry_name, directory_options)?
        } else {
            let options = if entry_name == "PEX-INFO" {
                other_file_options
            } else {
                zstd_file_options
            };
            dst_zip.start_file(entry_name, options)?;
            io::copy(&mut src_file, &mut dst_zip)?;
        }
    }

    let mut resources = embedded::RESOURCES;
    resources.inject_zip(&mut dst_zip, zstd_file_options)?;

    let file_options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    dst_zip.add_directory("__pex__/.clib", directory_options)?;
    info!("Embedded clibs:");
    for file in CLIBS_DIR.files() {
        let path = file.path();
        if let Some(clibs) = clibs.as_ref()
            && !clibs.contains(path)
        {
            continue;
        }
        let dst_path = format!(
            "__pex__/.clib/{clib}",
            clib = path
                .to_str()
                .expect("Embedded C-lib file names are utf-8 strings.")
        );
        anstream::eprint!(
            "Writing {entry} {size} bytes to {dst_path}...",
            entry = path.display().blue(),
            size = file.contents().len()
        );
        dst_zip.start_file(dst_path, file_options)?;
        let mut clib_reader = zstd::Decoder::new(file.contents())?;
        io::copy(&mut clib_reader, &mut dst_zip)?;
        anstream::eprintln!("{}.", "done".green())
    }

    dst_zip.start_file("__pex__/__init__.py", file_options)?;
    dst_zip.write_all(MAIN)?;
    dst_zip.start_file("__main__.py", file_options)?;
    dst_zip.write_all(MAIN)?;

    dst_zip.finish()?;
    mark_executable(dst_zip_fp.as_file_mut())?;
    dst_zip_fp.persist(pex.with_extension("pexrc"))?;

    Ok(())
}
