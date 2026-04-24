// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::io;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use boot::{inject_boot, sh_boot_shebang, write_boot};
use cache::{DigestingReader, Fingerprint, default_digest};
use fs_err as fs;
use fs_err::File;
use indexmap::IndexMap;
use log::info;
use owo_colors::OwoColorize;
use pex::{Layout, Pex};
use platform::mark_executable;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use scripts::Scripts;
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::embeds::{CLIBS_DIR, PROXIES_DIR};

pub fn inject_all(
    pexes: Vec<PathBuf>,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    clibs: Option<&HashSet<&Path>>,
    proxies: Option<&HashSet<&Path>>,
) -> anyhow::Result<()> {
    for pex in pexes {
        inject(&pex, compression_method, compression_level, clibs, proxies)?
    }
    Ok(())
}

fn inject(
    pex: &Path,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    clibs: Option<&HashSet<&Path>>,
    proxies: Option<&HashSet<&Path>>,
) -> anyhow::Result<()> {
    let pex = Pex::load(pex)?;
    match pex.layout {
        Layout::Loose | Layout::Packed => {
            inject_pex_dir(pex, compression_method, compression_level, clibs, proxies)
        }
        Layout::ZipApp => {
            inject_pex_zip(pex, compression_method, compression_level, clibs, proxies)
        }
    }
}

fn inject_pex_dir(
    mut pex: Pex,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    clibs: Option<&HashSet<&Path>>,
    proxies: Option<&HashSet<&Path>>,
) -> anyhow::Result<()> {
    // Make sure we have a shebang early. This partially validates the pex to inject is a valid one
    // before expending too much effort copying files below.
    let shebang = if let Some(sh_boot_shebang) = sh_boot_shebang(pex.path, true)? {
        sh_boot_shebang
    } else {
        let original_main = pex.path.join("__main__.py");
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

    let mut dest_pex = tempfile::tempdir_in(pex.path.parent().unwrap_or_else(|| Path::new(".")))?;
    let excludes: HashSet<PathBuf> = [
        ".bootstrap",
        ".deps",
        "PEX-INFO",
        "__main__.py",
        "__pex__",
        "__pycache__",
        "pex",
        "pex-repl",
    ]
    .into_iter()
    .map(|rel_path| pex.path.join(rel_path))
    .collect();
    for entry in walkdir::WalkDir::new(pex.path)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !excludes.contains(entry.path()))
    {
        let entry = entry?;
        let dst = dest_pex.path().join(entry.path().strip_prefix(pex.path)?);
        if entry.path().is_dir() {
            fs::create_dir_all(dst)?;
        } else {
            fs::copy(entry.path(), dst)?;
        }
    }
    let deps_dir = dest_pex.path().join(".deps");
    pex::repackage_wheels(&pex, compression_method, compression_level, &deps_dir)?;
    pex.info.deps_are_wheel_files = true;
    let wheel_file_names = pex.info.distributions.into_keys().collect::<Vec<_>>();
    pex.info.distributions = wheel_file_names
        .into_par_iter()
        .map(|wheel_file_name| {
            let fingerprint = Fingerprint::try_from(BufReader::new(File::open(
                deps_dir.join(&wheel_file_name),
            )?))?;
            Ok((wheel_file_name, fingerprint.hex_digest()))
        })
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .collect();

    let mut scripts = Scripts::Embedded;
    let pex_dir = dest_pex.path().join("__pex__");
    fs::create_dir_all(&pex_dir)?;
    scripts.write(dest_pex.path())?;

    let dst = pex.path.with_extension("pexrc");
    let clib_dir = pex_dir.join(".clibs");
    fs::create_dir_all(&clib_dir)?;
    info!("Embedded clibs:");
    for file in CLIBS_DIR.files() {
        let path = file.path();
        if let Some(clibs) = clibs.as_ref()
            && !clibs.contains(path)
        {
            continue;
        }
        embed_in_dir(path, file.contents(), &clib_dir, false)?;
    }
    let scripts_dir = pex_dir.join(".proxies");
    fs::create_dir_all(&scripts_dir)?;
    info!("Embedded proxies:");
    for file in PROXIES_DIR.files() {
        let path = file.path();
        if let Some(proxies) = proxies.as_ref()
            && !proxies.contains(path)
        {
            continue;
        }
        embed_in_dir(path, file.contents(), &scripts_dir, true)?;
    }

    pex.info
        .write(&mut File::create_new(dest_pex.path().join("PEX-INFO"))?)?;

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

fn embed_in_dir(
    path: &Path,
    contents: &[u8],
    dst_dir: &Path,
    mark_executable: bool,
) -> anyhow::Result<()> {
    let dst_path = dst_dir.join(path.file_name().expect("Embeds have file names."));
    anstream::eprint!(
        "Writing {entry} {size} bytes to {dst_path}...",
        entry = path.display().blue(),
        size = contents.len(),
        dst_path = dst_path.display(),
    );
    let mut dst_file = File::create_new(dst_path)?;
    let mut embed_reader = zstd::Decoder::new(contents)?;
    io::copy(&mut embed_reader, &mut dst_file)?;
    if mark_executable {
        platform::mark_executable(dst_file.file_mut())?;
    }
    anstream::eprintln!("{}.", "done".green());
    Ok(())
}

fn inject_pex_zip(
    mut pex: Pex,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    clibs: Option<&HashSet<&Path>>,
    proxies: Option<&HashSet<&Path>>,
) -> anyhow::Result<()> {
    let zip_read_fp = File::open(pex.path)?;
    let mut src_zip = ZipArchive::new(&zip_read_fp)?;
    let prefix = if let Some(sh_boot_shebang) = sh_boot_shebang(pex.path, false)? {
        Some(sh_boot_shebang.into_bytes())
    } else {
        let first_entry = src_zip.by_index(0)?;
        let zip_start = first_entry.header_start();
        if zip_start > 0 {
            let mut prefix_reader = File::open(pex.path)?.take(zip_start);
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

    let mut dst_zip_fp = if let Some(parent_dir) = pex.path.parent() {
        NamedTempFile::new_in(parent_dir)?
    } else {
        NamedTempFile::new()?
    };
    if let Some(prefix) = prefix {
        dst_zip_fp.write_all(&prefix)?;
    }
    let mut dst_zip = ZipWriter::new(&dst_zip_fp);

    let zstd_file_options = SimpleFileOptions::default()
        .compression_method(compression_method)
        .compression_level(compression_level);
    let other_file_options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let directory_options = SimpleFileOptions::default();
    for index in 0..src_zip.len() {
        let mut src_file = src_zip.by_index(index)?;
        let entry_name = src_file.name();
        if [".bootstrap/", ".deps/", "PEX-INFO", "__pex__/"]
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

    let deps_dir = tempfile::tempdir_in(pex.path.parent().unwrap_or_else(|| Path::new(".")))?;
    let stored_file_options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    pex::repackage_wheels(&pex, compression_method, compression_level, deps_dir.path())?;
    pex.info.deps_are_wheel_files = true;
    let mut distributions = IndexMap::with_capacity(pex.info.distributions.len());
    for wheel_file_name in pex.info.distributions.into_keys() {
        dst_zip.start_file(format!(".deps/{wheel_file_name}"), stored_file_options)?;
        let mut digesting_reader = DigestingReader::new(
            default_digest(),
            File::open(deps_dir.path().join(&wheel_file_name))?,
        );
        io::copy(&mut digesting_reader, &mut dst_zip)?;
        distributions.insert(
            wheel_file_name,
            digesting_reader.into_fingerprint().hex_digest(),
        );
    }
    pex.info.distributions = distributions;

    dst_zip.add_directory("__pex__", directory_options)?;
    Scripts::Embedded.inject(&mut dst_zip, zstd_file_options)?;

    let deflate_options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    dst_zip.add_directory("__pex__/.clibs", directory_options)?;
    info!("Embedded clibs:");
    for file in CLIBS_DIR.files() {
        let path = file.path();
        if let Some(clibs) = clibs.as_ref()
            && !clibs.contains(path)
        {
            continue;
        }
        embed_in_zip(
            path,
            file.contents(),
            &mut dst_zip,
            "__pex__/.clibs",
            deflate_options,
        )?;
    }
    dst_zip.add_directory("__pex__/.proxies", directory_options)?;
    info!("Embedded proxies:");
    for file in PROXIES_DIR.files() {
        let path = file.path();
        if let Some(proxies) = proxies.as_ref()
            && !proxies.contains(path)
        {
            continue;
        }
        embed_in_zip(
            path,
            file.contents(),
            &mut dst_zip,
            "__pex__/.proxies",
            zstd_file_options,
        )?;
    }

    dst_zip.start_file("PEX-INFO", other_file_options)?;
    pex.info.write(&mut dst_zip)?;

    inject_boot(&mut dst_zip, deflate_options)?;

    dst_zip.finish()?;
    mark_executable(dst_zip_fp.as_file_mut())?;

    let dst = pex.path.with_extension("pexrc");
    if dst.is_dir() {
        fs::remove_dir_all(&dst)?;
    }
    dst_zip_fp.persist(dst)?;

    Ok(())
}

fn embed_in_zip(
    path: &Path,
    contents: &[u8],
    dst_zip: &mut ZipWriter<impl Write + Seek>,
    dst_dir: &str,
    file_options: SimpleFileOptions,
) -> anyhow::Result<()> {
    let dst_path = format!(
        "{dst_dir}/{embed}",
        embed = path
            .file_name()
            .expect("Embeds have file names.")
            .to_str()
            .expect("Embed file names are utf-8 strings.")
    );
    anstream::eprint!(
        "Writing {entry} {size} bytes to {dst_path}...",
        entry = path.display().blue(),
        size = contents.len()
    );
    dst_zip.start_file(dst_path, file_options)?;
    let mut embed_reader = zstd::Decoder::new(contents)?;
    io::copy(&mut embed_reader, dst_zip)?;
    anstream::eprintln!("{}.", "done".green());
    Ok(())
}
