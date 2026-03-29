// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::io;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use boot::{inject_boot, sh_boot_shebang, write_boot};
use fs_err as fs;
use fs_err::File;
use log::info;
use owo_colors::OwoColorize;
use pex::{Layout, Pex};
use platform::mark_executable;
use scripts::Scripts;
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::clibs::CLIBS_DIR;

pub fn inject(
    pex: &Path,
    compression_level: Option<i64>,
    clibs: Option<&HashSet<&Path>>,
) -> anyhow::Result<()> {
    let pex = Pex::load(pex)?;
    match pex.layout {
        Layout::Loose | Layout::Packed => inject_pex_dir(pex.path, clibs),
        Layout::ZipApp => inject_pex_zip(pex.path, compression_level, clibs),
    }
}

fn inject_pex_dir(pex: &Path, clibs: Option<&HashSet<&Path>>) -> anyhow::Result<()> {
    // Make sure we have a shebang early. This partially validates the pex to inject is a valid one
    // before expending too much effort copying files below.
    let shebang = if let Some(sh_boot_shebang) = sh_boot_shebang(pex, true)? {
        sh_boot_shebang
    } else {
        let original_main = pex.join("__main__.py");
        io::BufReader::new(File::open(&original_main)?)
            .lines()
            .next()
            .ok_or_else(|| {
                anyhow!(
                    "Expected original PEX __main__.py to have a shebang line but {path} did not.",
                    path = original_main.display()
                )
            })??
    };

    let mut dest_pex = tempfile::tempdir_in(pex.parent().unwrap_or_else(|| Path::new(".")))?;
    let excludes: HashSet<PathBuf> = [
        ".bootstrap",
        "__main__.py",
        "__pex__",
        "__pycache__",
        "pex",
        "pex-repl",
    ]
    .into_iter()
    .map(|rel_path| pex.join(rel_path))
    .collect();
    for entry in walkdir::WalkDir::new(pex)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !excludes.contains(entry.path()))
    {
        let entry = entry?;
        let dst = dest_pex.path().join(entry.path().strip_prefix(pex)?);
        if entry.path().is_dir() {
            fs::create_dir_all(dst)?;
        } else {
            fs::copy(entry.path(), dst)?;
        }
    }

    let mut resources = Scripts::Embedded;
    let pex_dir = dest_pex.path().join("__pex__");
    fs::create_dir_all(&pex_dir)?;
    resources.write_scripts(dest_pex.path())?;

    let dst = pex.with_extension("pexrc");
    let clib_dir = pex_dir.join(".clib");
    fs::create_dir_all(&clib_dir)?;

    info!("Embedded clibs:");
    for file in CLIBS_DIR.files() {
        let path = file.path();
        if let Some(clibs) = clibs.as_ref()
            && !clibs.contains(path)
        {
            continue;
        }

        let dst_path = clib_dir.join(path);
        anstream::eprint!(
            "Writing {entry} {size} bytes to {dst_path}...",
            entry = path.display().blue(),
            size = file.contents().len(),
            dst_path = dst.join("__pex__").join(".clib").join(path).display(),
        );
        let mut dst_file = File::create_new(dst_path)?;
        let mut clib_reader = zstd::Decoder::new(file.contents())?;
        io::copy(&mut clib_reader, &mut dst_file)?;
        anstream::eprintln!("{}.", "done".green())
    }

    write_boot(dest_pex.path(), &shebang)?;

    if dst.is_dir() {
        fs::remove_dir_all(&dst)?;
    } else if dst.is_file() {
        fs::remove_file(&dst)?;
    }
    fs::rename(dest_pex.path(), dst)?;
    dest_pex.disable_cleanup(true);
    Ok(())
}

fn inject_pex_zip(
    pex: &Path,
    compression_level: Option<i64>,
    clibs: Option<&HashSet<&Path>>,
) -> anyhow::Result<()> {
    let zip_read_fp = File::open(pex)?;
    let mut src_zip = ZipArchive::new(&zip_read_fp)?;
    let prefix = if let Some(sh_boot_shebang) = sh_boot_shebang(pex, false)? {
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

    let mut resources = Scripts::Embedded;
    dst_zip.add_directory("__pex__", directory_options)?;
    resources.inject_scripts(&mut dst_zip, zstd_file_options)?;

    let deflate_options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
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
        dst_zip.start_file(dst_path, deflate_options)?;
        let mut clib_reader = zstd::Decoder::new(file.contents())?;
        io::copy(&mut clib_reader, &mut dst_zip)?;
        anstream::eprintln!("{}.", "done".green())
    }
    inject_boot(&mut dst_zip, deflate_options)?;

    dst_zip.finish()?;
    mark_executable(dst_zip_fp.as_file_mut())?;

    let dst = pex.with_extension("pexrc");
    if dst.is_dir() {
        fs::remove_dir_all(&dst)?;
    }
    dst_zip_fp.persist(dst)?;

    Ok(())
}
