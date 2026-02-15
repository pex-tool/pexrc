// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, io};

use anyhow::{anyhow, bail};
use log::info;
use logging_timer::time;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use zip::ZipArchive;

#[derive(strum_macros::Display, Eq, PartialEq)]
pub enum Algorithm {
    TryForEach,
    TryForEachInit,
}

#[time("debug", "{}")]
pub fn boot(
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path> + Sync + Send,
    argv: Vec<String>,
    algorithm: Option<Algorithm>,
    gc: bool,
) -> anyhow::Result<ExitStatus> {
    info!(
        "boot({python}, {pex}, {argv:?})",
        python = python.as_ref().display(),
        pex = pex.as_ref().display(),
        argv = argv
    );
    let pex_zip = ZipArchive::new(File::open(pex.as_ref())?)?;
    let metadata = pex_zip.metadata();
    let zip_entry_iter = (0..pex_zip.len()).into_par_iter();

    let zip_open_count = AtomicUsize::new(0);

    let dst_dir = tempfile::Builder::new().disable_cleanup(!gc).tempdir()?;
    match algorithm.unwrap_or(Algorithm::TryForEach) {
        Algorithm::TryForEach => zip_entry_iter.try_for_each(|index| -> anyhow::Result<()> {
            let zfp = File::open(pex.as_ref())?;
            let mut zr = unsafe { ZipArchive::unsafe_new_with_metadata(zfp, metadata.clone()) };
            zip_open_count.fetch_add(1, Ordering::Relaxed);
            extract_idx(&dst_dir, index, &mut zr)?;
            Ok(())
        })?,
        Algorithm::TryForEachInit => zip_entry_iter.try_for_each_init(
            || unsafe {
                File::open(pex.as_ref())
                    .map(|fp| ZipArchive::unsafe_new_with_metadata(fp, metadata.clone()))
                    .inspect(|_| {
                        zip_open_count.fetch_add(1, Ordering::Relaxed);
                    })
            },
            |open_result, index| -> anyhow::Result<()> {
                open_result
                    .as_mut()
                    .map(|zr| extract_idx(&dst_dir, index, zr))
                    .map_err(|err| anyhow::Error::msg(err.to_string()))?
            },
        )?,
    }

    eprintln!(
        "Extracted to {path} using {zip_open_count} zip opens and {thread_count} threads.",
        path = dst_dir.path().display(),
        zip_open_count = zip_open_count.load(Ordering::SeqCst),
        thread_count = rayon::current_num_threads(),
    );
    // TODO: execv for unix.
    Command::new(python.as_ref())
        .args(python_args)
        .arg(dst_dir.path())
        .args(argv)
        .spawn()?
        .wait()
        .map_err(|err| anyhow!("{err}"))
}

fn extract_idx(
    dst_dir: impl AsRef<Path>,
    index: usize,
    zr: &mut ZipArchive<File>,
) -> anyhow::Result<()> {
    let mut zip_file = zr.by_index(index)?;
    let dst_path = dst_dir.as_ref().join(zip_file.name());
    if zip_file.is_dir() {
        fs::create_dir_all(dst_path)?;
    } else {
        if let Some(parent_dir) = dst_path.parent() {
            fs::create_dir_all(parent_dir)?;
        }
        let mut dst_file = File::create_new(dst_path)?;
        io::copy(&mut zip_file, &mut dst_file)?;
    }
    Ok(())
}

#[time("debug", "{}")]
pub fn mount(
    _python: impl AsRef<Path>,
    _pex: impl AsRef<Path> + Sync + Send,
) -> anyhow::Result<PathBuf> {
    bail!("TODO: XXX")
}
