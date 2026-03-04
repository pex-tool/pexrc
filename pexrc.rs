// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::{cmp, io};

use anyhow::Context;
use clap::{Parser, Subcommand};
use include_dir::{Dir, include_dir};
use log::info;
use owo_colors::OwoColorize as _;
use platform::mark_executable;
use python::{ResourcePath, Resources, embedded};
use strum::IntoEnumIterator;
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const CLIBS_DIR: Dir<'_> = include_dir!("$CLIBS_DIR");
const MAIN: &[u8] = include_bytes!("python/pexrc/__init__.py");

/// Pex Runtime Control.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inject a traditional PEX with the pexrc runtime.
    Inject {
        #[arg(long)]
        compression_level: Option<i64>,

        #[arg(value_name = "FILE")]
        pex: PathBuf,
    },
    Info,
}

fn inject(pex: &Path, compression_level: Option<i64>) -> anyhow::Result<()> {
    let zip_read_fp = File::open(pex)?;

    let mut src_zip = ZipArchive::new(&zip_read_fp)?;
    let prefix = {
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

    let file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Zstd)
        .compression_level(compression_level);
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
            dst_zip.start_file(entry_name, file_options)?;
            io::copy(&mut src_file, &mut dst_zip)?;
        }
    }

    let mut resources = embedded::RESOURCES;
    dst_zip.add_directory("__pex__/.scripts", directory_options)?;
    for resource_path in ResourcePath::iter() {
        let text = resources.read(resource_path)?;
        dst_zip.start_file(
            format!(
                "__pex__/.scripts/{script}",
                script = resource_path.script_name()
            ),
            file_options,
        )?;
        dst_zip.write_all(text.as_bytes())?;
    }

    let file_options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    dst_zip.add_directory("__pex__/.clib", directory_options)?;
    info!("Embedded clibs:");
    for file in CLIBS_DIR.files() {
        let dst_path = format!("__pex__/.clib/{clib}", clib = file.path().display());
        anstream::eprint!(
            "Writing {entry} {size} bytes to {dst_path}...",
            entry = file.path().display().blue(),
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbosity.into())
        .init();
    cli.color.write_global();

    match cli.command {
        Commands::Inject {
            pex,
            compression_level,
        } => inject(&pex, compression_level),
        Commands::Info => {
            let mut paths = Vec::new();
            let mut max_width = 0;
            for clib in CLIBS_DIR.files() {
                let path = clib.path().display().to_string();
                max_width = cmp::max(max_width, path.len());
                paths.push(path);
            }
            let count = paths.len();
            anstream::println!(
                "There are {count} embedded {clibs}:",
                count = count.yellow(),
                clibs = if count == 1 { "clib" } else { "clibs" }
            );
            for (idx, (clib, path)) in CLIBS_DIR.files().zip(paths).enumerate() {
                anstream::println!(
                    "{idx:>3}. {path} {pad}{size:<7} bytes",
                    idx = (idx + 1).yellow(),
                    path = path.blue(),
                    pad = " ".repeat(max_width - path.len()),
                    size = clib.contents().len().yellow()
                )
            }
            Ok(())
        }
    }
}
