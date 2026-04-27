// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;

use chrono::Utc;
use fs_err::File;
use ouroboros::self_referencing;
use serde::Deserialize;
use zip::write::{FileOptions, SimpleFileOptions};

#[derive(Copy, Clone, Debug, Deserialize)]
struct DateTime(u16, u8, u8, u8, u8, u8);

impl DateTime {
    fn as_zip_date_time(&self) -> anyhow::Result<zip::DateTime> {
        let (year, month, day, hour, minute, second) =
            (self.0, self.1, self.2, self.3, self.4, self.5);
        Ok(zip::DateTime::from_date_and_time(
            year, month, day, hour, minute, second,
        )?)
    }
}

#[derive(Deserialize)]
struct RawOriginalWheelInfo<'a> {
    entries: Vec<(&'a str, DateTime, u32)>,
    filename: &'a str,
}

#[self_referencing]
pub(crate) struct OriginalWheelInfo {
    data: Vec<u8>,
    #[borrows(data)]
    #[covariant]
    info: RawOriginalWheelInfo<'this>,
}

impl OriginalWheelInfo {
    pub(crate) const fn file_name() -> &'static str {
        "original-whl-info.json"
    }

    pub(crate) fn load_from_dir(dir: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        let path = dir.as_ref().join(Self::file_name());
        Ok(if path.exists() {
            let mut file = File::open(path)?;
            let metadata = file.metadata()?;
            return Self::read(&mut file, metadata.len()).map(Some);
        } else {
            None
        })
    }

    pub(crate) fn read(mut contents: impl Read, size: u64) -> anyhow::Result<Self> {
        let mut data = Vec::with_capacity(usize::try_from(size)?);
        contents.read_to_end(&mut data)?;
        Ok(OriginalWheelInfo::try_new(data, |data| {
            serde_json::from_slice(data)
        })?)
    }

    pub(crate) fn filename(&self) -> &str {
        self.borrow_info().filename
    }

    pub(crate) fn iter_file_options(
        &self,
        base_options: SimpleFileOptions,
        timestamp: Option<chrono::DateTime<Utc>>,
    ) -> anyhow::Result<Vec<(&str, FileOptions<'static, ()>)>> {
        self.borrow_info()
            .entries
            .iter()
            .map(|(name, last_modified, external_attr)| {
                let mtime = if let Some(timestamp) = timestamp {
                    zip::DateTime::try_from(timestamp.naive_utc())?
                } else {
                    last_modified.as_zip_date_time()?
                };
                Ok((
                    *name,
                    base_options
                        .last_modified_time(mtime)
                        .unix_permissions(external_attr >> 16),
                ))
            })
            .collect::<anyhow::Result<_>>()
    }
}
