// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use fs_err::File;
use pex::{WheelFile, WheelOptions, recompress_zipped_whl};
use zip::ZipArchive;

pub fn repackage_all(
    wheels: Vec<PathBuf>,
    options: &WheelOptions,
    dest_dir: &Path,
) -> anyhow::Result<()> {
    for wheel in wheels {
        repackage(&wheel, options, dest_dir)?;
    }
    Ok(())
}

fn repackage(wheel: &Path, options: &WheelOptions, dest_dir: &Path) -> anyhow::Result<()> {
    let wheel_file_name = wheel
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .ok_or_else(|| {
            anyhow!(
                "Wheel does not have a (UTF-8) filename!: {wheel}",
                wheel = wheel.display()
            )
        })?;
    let wheel_file = WheelFile::parse_file_name(wheel_file_name)?;
    let zipped_whl = ZipArchive::new(File::open(wheel)?)?;
    recompress_zipped_whl(zipped_whl, &wheel_file, options, dest_dir)?;
    Ok(())
}
