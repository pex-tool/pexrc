// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::ffi::OsStr;
use std::io;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::bail;
use fs_err as fs;
use fs_err::File;
use logging_timer::time;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use zip::read::ZipArchiveMetadata;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::wheel::WheelFile;
use crate::wheel::layout::WheelLayout;
use crate::wheel::original_wheel_info::OriginalWheelInfo;
use crate::wheel::record::Record;
use crate::{Layout, Pex};

#[derive(Copy, Clone)]
enum DirPexDepType {
    Chroot,
    OriginalWhl,
    ZippedChroot,
}

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
            let dep_type = if pex.info.deps_are_wheel_files {
                DirPexDepType::OriginalWhl
            } else if matches!(pex.layout, Layout::Packed) {
                DirPexDepType::ZippedChroot
            } else {
                DirPexDepType::Chroot
            };
            wheel_files
                .into_par_iter()
                .map(|wheel_file: WheelFile| {
                    repackage_directory_pex_wheel(
                        pex.path,
                        &wheel_file,
                        dep_type,
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
        let wheel_prefix = format!(
            ".deps/{wheel_file_name}",
            wheel_file_name = wheel_file.file_name
        );
        recompress_zipped_whl(
            ZipArchive::new(pex_zip_fp.by_name_seek(&wheel_prefix)?)?,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        )
    } else {
        recompress_zipped_whl_chroot(
            pex_zip_fp,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
            true,
        )
    }
}

fn repackage_directory_pex_wheel(
    pex_dir: &Path,
    wheel_file: &WheelFile,
    dep_type: DirPexDepType,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    let wheel_path = pex_dir.join(".deps").join(wheel_file.file_name);
    match dep_type {
        DirPexDepType::Chroot => compress_whl_chroot(
            &wheel_path,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        ),
        DirPexDepType::OriginalWhl => recompress_zipped_whl(
            ZipArchive::new(File::open(wheel_path)?)?,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
        ),
        DirPexDepType::ZippedChroot => recompress_zipped_whl_chroot(
            ZipArchive::new(File::open(wheel_path)?)?,
            wheel_file,
            compression_method,
            compression_level,
            dest_dir,
            false,
        ),
    }
}

fn recompress_zipped_whl(
    mut wheel: ZipArchive<impl Read + Seek>,
    wheel_file: &WheelFile,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    fs::create_dir_all(dest_dir)?;
    let dest_wheel = dest_dir.join(wheel_file.file_name);
    let compressed = File::create(&dest_wheel)?;
    let mut compressed_whl = ZipWriter::new(compressed);
    for index in 0..wheel.len() {
        let entry = wheel.by_index_raw(index)?;
        if entry.name().ends_with(".pyc") {
            continue;
        }
        if entry.compression() == compression_method {
            compressed_whl.raw_copy_file(entry)?;
        } else if entry.is_dir() {
            compressed_whl.add_directory(entry.name(), entry.options())?;
        } else {
            drop(entry);
            let mut entry = wheel.by_index(index)?;
            // N.B.: entry.options is actually lossy (loses high bits); so we can't round-trip
            // more exotic permissions faithfully currently. An example of this is the cowsay 6.1
            // wheel on PyPi whose RECORD has 0o100664 which gets truncated to 0o644. Note also
            // though that `raw_copy_file` (used above when no transcoding is needed) _does_
            // preserve these bits.
            // See: https://github.com/zip-rs/zip2/issues/433
            compressed_whl.start_file(
                entry.name(),
                entry
                    .options()
                    .compression_method(compression_method)
                    .compression_level(compression_level),
            )?;

            io::copy(&mut entry, &mut compressed_whl)?;
        }
    }
    compressed_whl.finish()?;
    Ok(File::open(dest_wheel)?)
}

fn recompress_zipped_whl_chroot(
    mut zipped_wheel_chroot: ZipArchive<impl Read + Seek>,
    wheel_file: &WheelFile,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
    prefixed: bool,
) -> anyhow::Result<File> {
    fs::create_dir_all(dest_dir)?;
    let file_options = SimpleFileOptions::default()
        .compression_method(compression_method)
        .compression_level(compression_level);

    let prefix = if prefixed {
        format_args!(
            ".deps/{wheel_file_name}/",
            wheel_file_name = wheel_file.file_name
        )
    } else {
        format_args!("")
    };

    let record_name = format!(
        "{prefix}{dist_info_dir}/RECORD",
        dist_info_dir = wheel_file.dist_info_dir().display()
    );
    let record = Record::read(Cursor::new(io::read_to_string(
        zipped_wheel_chroot.by_name(&record_name)?,
    )?))?;

    let (stash_dir, legacy_bin_dir) = 'result: {
        let layout_json_name = if prefixed {
            Cow::Owned(format!(
                "{prefix}{file_name}",
                file_name = WheelLayout::file_name()
            ))
        } else {
            Cow::Borrowed(WheelLayout::file_name())
        };
        match zipped_wheel_chroot.by_name(layout_json_name.as_ref()) {
            Ok(zip_file) => {
                let layout = WheelLayout::read(zip_file)?;
                break 'result (Some(layout.stash_dir), false);
            }
            Err(ZipError::FileNotFound) => {}
            Err(err) => bail!("{err}"),
        }
        let legacy_bin_dir_name = if prefixed {
            Cow::Owned(format!("{prefix}bin"))
        } else {
            Cow::Borrowed("bin")
        };
        let has_legacy_bin_dir = !record.wheel_has_bin_dir()
            && zipped_wheel_chroot
                .by_name(legacy_bin_dir_name.as_ref())
                .ok()
                .map(|entry| entry.is_dir())
                .unwrap_or_default();
        (None, has_legacy_bin_dir)
    };

    let data_dir = wheel_file.data_dir();
    let original_wheel_info = format!(
        "{prefix}{pex_info_dir}/{file_name}",
        pex_info_dir = wheel_file.pex_info_dir().display(),
        file_name = OriginalWheelInfo::file_name()
    );

    let wheel_info = if let Ok(wheel_info) = zipped_wheel_chroot.by_name(&original_wheel_info) {
        let size = wheel_info.size();
        Some(OriginalWheelInfo::read(wheel_info, size)?)
    } else {
        None
    };

    let (dest_wheel, compressed_whl) = if let Some(wheel_info) = wheel_info {
        let dest_wheel = dest_dir.join(wheel_info.filename());
        let mut compressed_whl = ZipWriter::new(File::create(&dest_wheel)?);
        for (dst_rel_path, options) in wheel_info.iter_file_options(file_options)? {
            if dst_rel_path.ends_with(".pyc") {
                continue;
            }
            let name = 'result: {
                if let Ok(data_dir_rel_path) = Path::new(dst_rel_path).strip_prefix(&data_dir) {
                    if let Some(stash_dir) = stash_dir.as_deref() {
                        break 'result format!(
                            "{prefix}{stash_dir}/{rel_path}",
                            stash_dir = stash_dir.display(),
                            rel_path = normalized_data_dir_relpath(data_dir_rel_path).display()
                        );
                    }
                    if legacy_bin_dir {
                        let rel_path = normalized_data_dir_relpath(data_dir_rel_path);
                        assert!(starts_with(rel_path.as_ref(), "bin"));
                        break 'result format!(
                            "{prefix}{rel_path}",
                            rel_path = rel_path.as_ref().display()
                        );
                    }
                }
                format!("{prefix}{dst_rel_path}",)
            };
            let mut src = match zipped_wheel_chroot.by_name(&name) {
                Ok(src) => src,
                Err(_) if dst_rel_path.ends_with("/") => {
                    // N.B.: Pex can omit original directory entries when those directories are
                    // empty.
                    compressed_whl.add_directory(dst_rel_path, options)?;
                    continue;
                }
                Err(err) => bail!(
                    "Mapped {dst_rel_path} in {file_name} to {name} which was not found: {err}",
                    file_name = wheel_file.file_name
                ),
            };
            if src.is_dir() {
                compressed_whl.add_directory_from_path(dst_rel_path, options)?;
            } else {
                compressed_whl.start_file_from_path(dst_rel_path, options)?;
                if src.name() == record_name {
                    compressed_whl.write_all(
                        record
                            .filtered(
                                wheel_file,
                                stash_dir.as_deref(),
                                if legacy_bin_dir {
                                    Some(Path::new("bin"))
                                } else {
                                    None
                                },
                            )?
                            .as_slice(),
                    )?;
                } else {
                    io::copy(&mut src, &mut compressed_whl)?;
                }
            }
        }
        (dest_wheel, compressed_whl)
    } else {
        let dest_wheel = dest_dir.join(wheel_file.file_name);
        let mut compressed_whl = ZipWriter::new(File::create(&dest_wheel)?);
        for entry in record.entries() {
            let dst_rel_path = entry.path.as_ref();
            let mut src = 'result: {
                if let Ok(data_dir_rel_path) = dst_rel_path.strip_prefix(&data_dir) {
                    if let Some(stash_dir) = stash_dir.as_deref() {
                        break 'result zipped_wheel_chroot.by_name(&format!(
                            "{prefix}{stash_dir}/{rel_path}",
                            stash_dir = stash_dir.display(),
                            rel_path = normalized_data_dir_relpath(data_dir_rel_path).display()
                        ))?;
                    }
                    if legacy_bin_dir {
                        let rel_path = normalized_data_dir_relpath(data_dir_rel_path);
                        assert!(starts_with(rel_path.as_ref(), "bin"));
                        break 'result zipped_wheel_chroot.by_name(&format!(
                            "{prefix}{rel_path}",
                            rel_path = rel_path.as_ref().display()
                        ))?;
                    }
                }
                zipped_wheel_chroot.by_name(&format!(
                    "{prefix}{rel_path}",
                    rel_path = dst_rel_path.display()
                ))?
            };

            compressed_whl.start_file_from_path(dst_rel_path, file_options)?;
            io::copy(&mut src, &mut compressed_whl)?;
        }
        (dest_wheel, compressed_whl)
    };

    compressed_whl.finish()?;
    Ok(File::open(dest_wheel)?)
}

fn compress_whl_chroot(
    wheel_dir: &Path,
    wheel_file: &WheelFile,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
    dest_dir: &Path,
) -> anyhow::Result<File> {
    fs::create_dir_all(dest_dir)?;
    let file_options = SimpleFileOptions::default()
        .compression_method(compression_method)
        .compression_level(compression_level);

    let (record, record_rel_path) = Record::parse(wheel_dir, wheel_file)?;

    let (stash_dir, legacy_bin_dir) = 'result: {
        if let Some(layout) = WheelLayout::load_from_dir(wheel_dir)? {
            let stash_dir = wheel_dir.join(layout.stash_dir);
            if stash_dir.exists() {
                break 'result (Some(stash_dir), None);
            }
        }
        let bin_dir = wheel_dir.join("bin");
        if bin_dir.is_dir() && !record.wheel_has_bin_dir() {
            break 'result (None, Some(bin_dir));
        }
        (None, None)
    };

    let data_dir = wheel_file.data_dir();
    let pex_info_dir = wheel_dir.join(wheel_file.pex_info_dir());
    let (dest_wheel, compressed_whl) = if let Some(wheel_info) =
        OriginalWheelInfo::load_from_dir(pex_info_dir)?
    {
        let dest_wheel = dest_dir.join(wheel_info.filename());
        let mut compressed_whl = ZipWriter::new(File::create(&dest_wheel)?);
        for (dst_rel_path, options) in wheel_info.iter_file_options(file_options)? {
            if dst_rel_path.ends_with(".pyc") {
                continue;
            }
            let dst_rel_path = Path::new(dst_rel_path);
            let mut src = wheel_dir.join(dst_rel_path);
            if let Ok(data_dir_rel_path) = dst_rel_path.strip_prefix(&data_dir) {
                if let Some(stash_dir) = stash_dir.as_deref() {
                    src = stash_dir.join(normalized_data_dir_relpath(data_dir_rel_path))
                } else if let Some(bin_dir) = legacy_bin_dir.as_deref() {
                    let rel_path = normalized_data_dir_relpath(data_dir_rel_path);
                    assert!(starts_with(rel_path.as_ref(), "bin"));
                    src = bin_dir.join(rel_path)
                }
            }
            if src.is_dir() {
                compressed_whl.add_directory_from_path(dst_rel_path, options)?;
            } else {
                compressed_whl.start_file_from_path(dst_rel_path, options)?;
                if dst_rel_path == record_rel_path {
                    compressed_whl.write_all(
                        record
                            .filtered(
                                wheel_file,
                                stash_dir.as_deref().map(|dir| {
                                    dir.strip_prefix(wheel_dir)
                                        .expect("We appended the stash dir to the wheel dir above.")
                                }),
                                legacy_bin_dir.as_deref().map(|dir| {
                                    dir.strip_prefix(wheel_dir).expect(
                                        "We appended the legacy bin dir to the wheel dir above.",
                                    )
                                }),
                            )?
                            .as_slice(),
                    )?;
                } else {
                    io::copy(&mut File::open(src)?, &mut compressed_whl)?;
                }
            }
        }
        (dest_wheel, compressed_whl)
    } else {
        let dest_wheel = dest_dir.join(wheel_file.file_name);
        let mut compressed_whl = ZipWriter::new(File::create(&dest_wheel)?);
        for entry in record.entries() {
            let dst_rel_path = entry.path.as_ref();
            let mut src = wheel_dir.join(dst_rel_path);
            if let Ok(data_dir_rel_path) = dst_rel_path.strip_prefix(&data_dir) {
                if let Some(stash_dir) = stash_dir.as_deref() {
                    src = stash_dir.join(normalized_data_dir_relpath(data_dir_rel_path))
                } else if let Some(bin_dir) = legacy_bin_dir.as_deref() {
                    let rel_path = normalized_data_dir_relpath(data_dir_rel_path);
                    assert!(starts_with(rel_path.as_ref(), "bin"));
                    src = bin_dir.join(rel_path)
                }
            }
            compressed_whl.start_file_from_path(dst_rel_path, file_options)?;
            io::copy(&mut File::open(src)?, &mut compressed_whl)?;
        }
        (dest_wheel, compressed_whl)
    };

    compressed_whl.finish()?;
    Ok(File::open(dest_wheel)?)
}

fn starts_with(path: &Path, name: impl AsRef<OsStr>) -> bool {
    matches!(path.components().next(), Some(Component::Normal(named)) if named == name.as_ref())
}

fn normalized_data_dir_relpath(path: &Path) -> Cow<'_, Path> {
    let mut components = path.components();
    if let Some(start) = components.next()
        && matches!(start, Component::Normal(name) if name == "scripts")
    {
        Cow::Owned(
            [Component::Normal(OsStr::new("bin"))]
                .into_iter()
                .chain(components)
                .collect(),
        )
    } else {
        Cow::Borrowed(path)
    }
}
