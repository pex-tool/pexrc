// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::fs::File;
use std::path::Path;

pub fn atomic_file(
    path: &Path,
    func: impl FnOnce(&mut File) -> anyhow::Result<()>,
) -> anyhow::Result<File> {
    if path.is_file() {
        return Ok(File::open(path)?);
    }
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }
    let lock_file_path = path.with_added_extension("lck");
    {
        let mut lock_file = File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_file_path)?;
        lock_file.lock()?;
        if path.is_file() {
            return Ok(File::open(path)?);
        }
        func(&mut lock_file)?;
    }
    fs::rename(lock_file_path, path)?;
    Ok(File::open(path)?)
}
