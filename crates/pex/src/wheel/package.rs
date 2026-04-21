// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use csv::{Terminator, Trim};
use fs_err as fs;
use fs_err::File;
use logging_timer::time;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use zip::read::ZipArchiveMetadata;
use zip::write::{FileOptions, SimpleFileOptions};
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::wheel::WheelFile;
use crate::{EntryPoints, Layout, Pex};

#[time("debug", "{}")]
pub fn repackage_wheels(
    pex: &Pex,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<Vec<File>> {
    let wheel_files = pex
        .info
        .parse_distributions()
        .collect::<Result<Vec<_>, _>>()?;
    match pex.layout {
        Layout::Loose | Layout::Packed => {
            let is_whl_zip = pex.info.deps_are_wheel_files || pex.layout == Layout::Packed;
            wheel_files
                .into_par_iter()
                .map(|wheel_file: WheelFile| {
                    repackage_directory_pex_wheel(
                        pex.path,
                        &wheel_file,
                        is_whl_zip,
                        compression_method,
                        compression_level,
                        dest_dir,
                    )
                })
                .collect::<anyhow::Result<Vec<_>>>()
        }
        Layout::ZipApp => {
            let pex_zip = ZipArchive::new(File::open(pex.path)?)?;
            let zip_metadata = pex_zip.metadata();
            wheel_files
                .into_par_iter()
                .map(|wheel_file: WheelFile| {
                    repackage_zipapp_pex_wheel(
                        pex.path,
                        zip_metadata.clone(),
                        &wheel_file,
                        pex.info.deps_are_wheel_files,
                        compression_method,
                        compression_level,
                        dest_dir,
                    )
                })
                .collect::<anyhow::Result<Vec<_>>>()
        }
    }
}

fn repackage_zipapp_pex_wheel(
    pex_zip: &Path,
    zip_metadata: Arc<ZipArchiveMetadata>,
    wheel_file: &WheelFile,
    is_whl_zip: bool,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    let mut pex_zip_fp =
        unsafe { ZipArchive::unsafe_new_with_metadata(File::open(pex_zip)?, zip_metadata.clone()) };
    if is_whl_zip {
        recompress_whl(
            pex_zip_fp.by_name_seek(&format!(
                ".deps/{wheel_file_name}",
                wheel_file_name = wheel_file.file_name
            ))?,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        )
    } else {
        let wheel_prefix = format!(
            ".deps/{wheel_file_name}/",
            wheel_file_name = wheel_file.file_name
        );
        let extract_indices = (0..pex_zip_fp.len())
            .filter_map(|index| {
                pex_zip_fp.by_index(index).ok().and_then(|file| {
                    if file.name().starts_with(&wheel_prefix) {
                        Some(index)
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>();
        // TODO: XXX: Implement logic to go straight from input zip to output zip.
        fs::create_dir_all(dest_dir)?;
        let temp_dir = tempfile::tempdir_in(dest_dir)?;
        extract_indices.into_par_iter().try_for_each(|index| {
            extract_index(
                pex_zip,
                zip_metadata.clone(),
                index,
                &wheel_prefix,
                temp_dir.path(),
            )
        })?;
        compress_whl(
            temp_dir.path(),
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        )
    }
}

fn extract_index(
    pex_zip: &Path,
    zip_metadata: Arc<ZipArchiveMetadata>,
    index: usize,
    strip_prefix: &str,
    dest_dir: &Path,
) -> anyhow::Result<()> {
    let mut pex_zip_fp =
        unsafe { ZipArchive::unsafe_new_with_metadata(File::open(pex_zip)?, zip_metadata) };
    let mut entry = pex_zip_fp.by_index(index)?;

    let rel_path = entry.name().strip_prefix(strip_prefix).ok_or_else(|| {
        anyhow!(
            "Zip entry {name} in {pex} did not have expected prefix {strip_prefix}.",
            name = entry.name(),
            pex = pex_zip.display()
        )
    })?;
    #[cfg(unix)]
    let dest_path = dest_dir.join(rel_path);
    #[cfg(windows)]
    let dest_path = {
        let mut dest_path = Cow::Borrowed(dest_dir);
        for component in rel_path.split("/") {
            dest_path = Cow::Owned(dest_path.join(component))
        }
        dest_path.into_owned()
    };

    if entry.is_dir() {
        fs::create_dir_all(dest_path)?;
    } else {
        if let Some(parent_dir) = dest_path.parent() {
            fs::create_dir_all(parent_dir)?;
        }
        io::copy(&mut entry, &mut File::create_new(dest_path)?)?;
    }
    Ok(())
}

fn repackage_directory_pex_wheel(
    pex_dir: &Path,
    wheel_file: &WheelFile,
    is_whl_zip: bool,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    let wheel_path = pex_dir.join(".deps").join(wheel_file.file_name);
    if is_whl_zip {
        recompress_whl(
            File::open(wheel_path)?,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        )
    } else {
        compress_whl(
            &wheel_path,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        )
    }
}

fn recompress_whl(
    wheel: impl Read + Seek,
    wheel_file: &WheelFile,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    let mut whl = ZipArchive::new(wheel)?;
    // TODO: XXX: Implement logic to go straight from input zip to output zip.
    fs::create_dir_all(dest_dir)?;
    let temp_dir = tempfile::tempdir_in(dest_dir)?;
    whl.extract(temp_dir.path())?;
    compress_whl(
        temp_dir.path(),
        wheel_file,
        compression_method,
        compression_level,
        dest_dir,
    )
}

fn load_from_json<D: DeserializeOwned>(path: impl AsRef<Path>) -> anyhow::Result<Option<D>> {
    if path.as_ref().exists() {
        return Ok(Some(serde_json::from_reader(File::open(path.as_ref())?)?));
    }
    Ok(None)
}

#[derive(Deserialize)]
struct WheelLayout {
    stash_dir: PathBuf,
}

impl WheelLayout {
    fn load(path: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        load_from_json(path)
    }
}

#[derive(Deserialize)]
struct RecordEntry {
    path: String,
    _hash: String,
    _size: usize,
}

fn has_legacy_bin_dir(wheel_dir: &Path, wheel_file: &WheelFile) -> anyhow::Result<Option<PathBuf>> {
    let bin_dir = wheel_dir.join("bin");
    if bin_dir.is_dir() {
        for record in csv::ReaderBuilder::new()
            .quote(b'"')
            .delimiter(b',')
            .terminator(Terminator::CRLF)
            .trim(Trim::All)
            .from_path(wheel_dir.join(wheel_file.dist_info_dir()).join("RECORD"))?
            .into_deserialize()
        {
            let entry: RecordEntry = record?;
            // N.B.: The spec here is very poor:
            // https://packaging.python.org/en/latest/specifications/recording-installed-packages/#the-record-file
            // There is no such thing as "on Windows" since a non-platform-specific wheel could be
            // created on Windows or Unix and uploaded to a registry. That said, the occurrence of
            // a dir name like bin/suffix\\ on Windows or vice versa seems unlikely due to all the
            // problems it would cause the wheel author when people went to use it.
            if &entry.path == "bin"
                || entry.path.starts_with("bin/")
                || entry.path.starts_with("bin\\")
            {
                return Ok(None);
            }
        }
        Ok(Some(bin_dir))
    } else {
        Ok(None)
    }
}

#[derive(Copy, Clone, Debug, Deserialize)]
struct DateTime(u16, u8, u8, u8, u8, u8);

impl TryInto<zip::DateTime> for DateTime {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<zip::DateTime, Self::Error> {
        let (year, month, day, hour, minute, second) =
            (self.0, self.1, self.2, self.3, self.4, self.5);
        Ok(zip::DateTime::from_date_and_time(
            year, month, day, hour, minute, second,
        )?)
    }
}

#[derive(Deserialize)]
struct OriginalWheelInfo {
    entries: Vec<(PathBuf, DateTime, u32)>,
}

impl OriginalWheelInfo {
    fn load(path: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        load_from_json(path)
    }

    fn try_into_file_options(
        self,
        base_options: SimpleFileOptions,
    ) -> anyhow::Result<HashMap<PathBuf, FileOptions<'static, ()>>> {
        self.entries
            .into_iter()
            .map(|(name, last_modified, external_attr)| {
                last_modified.try_into().map(|date_time| {
                    let options = base_options
                        .last_modified_time(date_time)
                        .unix_permissions(external_attr >> 16);
                    (name, options)
                })
            })
            .collect::<anyhow::Result<_>>()
    }
}

fn load_entry_points(wheel_dir: &Path, wheel_file: &WheelFile) -> anyhow::Result<EntryPoints> {
    let entry_points = wheel_dir
        .join(wheel_file.dist_info_dir())
        .join("entry_points.txt");
    Ok(if entry_points.exists() {
        EntryPoints::load(File::open(entry_points)?)?
    } else {
        EntryPoints::empty()
    })
}

fn data_dir_relpath(
    wheel_dir: &Path,
    wheel_file: &WheelFile,
    stash_dir_relpath: &Path,
) -> anyhow::Result<(bool, PathBuf)> {
    let mut components = stash_dir_relpath.iter();
    let (is_scripts_dir, dir_name) = components
        .next()
        .map(|dir_name| {
            if dir_name == "bin" {
                (true, OsStr::new("scripts"))
            } else {
                (false, dir_name)
            }
        })
        .ok_or_else(|| {
            anyhow!(
                "Unexpected stash dir entry for {wheel} at {dir}: {entry}",
                wheel = wheel_file.file_name,
                dir = wheel_dir.display(),
                entry = stash_dir_relpath.display()
            )
        })?;
    Ok((
        is_scripts_dir,
        PathBuf::from_iter([dir_name].into_iter().chain(components)),
    ))
}

fn compress_whl(
    wheel_dir: &Path,
    wheel_file: &WheelFile,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    fs::create_dir_all(dest_dir)?;
    let dest_wheel = dest_dir.join(wheel_file.file_name);
    let compressed = File::create(&dest_wheel)?;
    let mut compressed_whl = ZipWriter::new(compressed);
    let directory_options = SimpleFileOptions::default();
    let file_options = SimpleFileOptions::default()
        .compression_method(compression_method)
        .compression_level(compression_level);

    let mut excludes: HashSet<PathBuf> = HashSet::new();

    let pex_info_dir = wheel_dir.join(wheel_file.pex_info_info_dir());
    let mut original_wheel_info = if let Some(wheel_info) =
        OriginalWheelInfo::load(pex_info_dir.join("original-whl-info.json"))?
    {
        wheel_info.try_into_file_options(file_options)?
    } else {
        HashMap::new()
    };
    excludes.insert(pex_info_dir);

    let layout_file = wheel_dir.join(".layout.json");
    let stash_dir = 'result: {
        if let Some(layout) = WheelLayout::load(&layout_file)? {
            let stash_dir = wheel_dir.join(layout.stash_dir);
            if stash_dir.exists() {
                break 'result Some(stash_dir);
            }
        }
        None
    };
    if let Some(stash_dir) = stash_dir {
        let entry_points = load_entry_points(wheel_dir, wheel_file)?;
        let data_dir = wheel_file.data_dir();
        let mut scripts = Vec::new();
        for entry in walkdir::WalkDir::new(&stash_dir).min_depth(1) {
            let entry = entry?;
            let stash_dir_relpath = entry.path().strip_prefix(&stash_dir).expect(
                "Walked unpacked wheel paths should be child paths of the unpacked wheel root dir.",
            );
            let (is_scripts_dir, relpath) =
                data_dir_relpath(wheel_dir, wheel_file, stash_dir_relpath)?;
            if entry.path().is_dir() {
                if is_scripts_dir {
                    continue;
                }
                compressed_whl
                    .add_directory_from_path(data_dir.join(relpath), directory_options)?;
            } else {
                let data_dir_path = data_dir.join(relpath);
                let options = original_wheel_info
                    .remove(&data_dir_path)
                    .unwrap_or(file_options);
                if is_scripts_dir {
                    match entry.file_name().to_str() {
                        None => scripts.push((File::open(entry.path())?, data_dir_path, options)),
                        Some(script_name) if !entry_points.is_script(script_name) => {
                            scripts.push((File::open(entry.path())?, data_dir_path, options));
                        }
                        _ => {}
                    }
                } else {
                    compressed_whl.start_file_from_path(data_dir_path, options)?;
                    io::copy(&mut File::open(entry.path())?, &mut compressed_whl)?;
                }
            }
        }
        if !scripts.is_empty() {
            compressed_whl.add_directory_from_path(data_dir.join("scripts"), directory_options)?;
            for (mut script_file, data_dir_path, options) in scripts {
                compressed_whl.start_file_from_path(data_dir_path, options)?;
                io::copy(&mut script_file, &mut compressed_whl)?;
            }
        }
        excludes.insert(stash_dir);
    } else if let Some(bin_dir) = has_legacy_bin_dir(wheel_dir, wheel_file)? {
        let entry_points = load_entry_points(wheel_dir, wheel_file)?;
        let dst_dir = wheel_file.data_dir().join("scripts");
        for entry in walkdir::WalkDir::new(&bin_dir).min_depth(1) {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && entry_points.is_script(name)
            {
                continue;
            }
            let dst_rel_path = dst_dir.join(entry.path().strip_prefix(&bin_dir).expect(
                "Walked unpacked wheel paths should be child paths of the unpacked wheel root dir.",
            ));
            if entry.path().is_dir() {
                compressed_whl.add_directory_from_path(dst_rel_path, directory_options)?;
            } else {
                let options = original_wheel_info
                    .remove(&dst_rel_path)
                    .unwrap_or(file_options);
                compressed_whl.start_file_from_path(dst_rel_path, options)?;
                io::copy(&mut File::open(entry.path())?, &mut compressed_whl)?;
            }
        }
        excludes.insert(bin_dir);
    }
    excludes.insert(layout_file);

    for entry in walkdir::WalkDir::new(wheel_dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !excludes.contains(entry.path()))
    {
        let entry = entry?;
        let dst_rel_path = entry.path().strip_prefix(wheel_dir).expect(
            "Walked unpacked wheel paths should be child paths of the unpacked wheel root dir.",
        );
        if entry.path().is_dir() {
            compressed_whl.add_directory_from_path(dst_rel_path, directory_options)?;
        } else {
            let options = original_wheel_info
                .remove(dst_rel_path)
                .unwrap_or(file_options);
            compressed_whl.start_file_from_path(dst_rel_path, options)?;
            io::copy(&mut File::open(entry.path())?, &mut compressed_whl)?;
        }
    }

    compressed_whl.finish()?;
    Ok(File::open(dest_wheel)?)
}
