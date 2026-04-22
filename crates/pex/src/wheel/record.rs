// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use csv::{StringRecord, Terminator, Trim};
use fs_err::File;
use ouroboros::self_referencing;

use crate::wheel::WheelFile;

pub(crate) struct Entry<'a> {
    pub(crate) path: PathBuf,
    pub(crate) _raw_path: &'a str,
    pub(crate) _hash: Option<&'a str>,
    pub(crate) _size: Option<usize>,
}

fn parse_entry_record<'a>(
    row: usize,
    record: Result<&'a StringRecord, &'a csv::Error>,
) -> Option<anyhow::Result<Entry<'a>>> {
    match record {
        Ok(record) => {
            if record.is_empty() {
                return None;
            }
            let fields = record.into_iter().collect::<Vec<_>>();
            match fields.as_slice() {
                &[_raw_path, hash, size] => {
                    let _hash = if hash.is_empty() { None } else { Some(hash) };
                    let _size = if size.is_empty() {
                        None
                    } else {
                        match size.parse::<usize>() {
                            Ok(size) => Some(size),
                            Err(err) => {
                                return Some(Err(anyhow!(
                                    "Row {row} has an invalid size entry ({record}): {err}",
                                    record = record.as_slice()
                                )));
                            }
                        }
                    };

                    // N.B.: The spec here is very poor:
                    // https://packaging.python.org/en/latest/specifications/recording-installed-packages/#the-record-file
                    // There is no such thing as "on Windows" since a non-platform-specific wheel
                    // could be created on Windows or Unix and uploaded to a registry. That said,
                    // the occurrence of a dir name like bin/suffix\\ on Windows or vice versa seems
                    // unlikely due to all the problems it would cause the wheel author when people
                    // went to use it.
                    let path = _raw_path.split("/").collect();

                    Some(Ok(Entry {
                        path,
                        _raw_path,
                        _hash,
                        _size,
                    }))
                }
                _ => Some(Err(anyhow!(
                    "Each row should have path,hash,size: row {row} is missing fields: {record}",
                    record = record.as_slice()
                ))),
            }
        }
        Err(err) => Some(Err(anyhow!("{err}"))),
    }
}

#[self_referencing]
pub(crate) struct Record {
    records: Vec<csv::Result<StringRecord>>,

    #[borrows(records)]
    #[covariant]
    entries: Vec<Entry<'this>>,
}

impl Record {
    pub(crate) fn parse(wheel_dir: &Path, wheel_file: &WheelFile) -> anyhow::Result<Self> {
        let records = csv::ReaderBuilder::new()
            .quote(b'"')
            .delimiter(b',')
            .terminator(Terminator::CRLF)
            .trim(Trim::All)
            .from_reader(File::open(
                wheel_dir.join(wheel_file.dist_info_dir()).join("RECORD"),
            )?)
            .into_records()
            .collect::<Vec<_>>();
        Record::try_new(records, |records| {
            records
                .iter()
                .enumerate()
                .filter_map(|(idx, record)| parse_entry_record(idx + 1, record.as_ref()))
                .collect()
        })
    }

    pub(crate) fn entries(&self) -> &[Entry<'_>] {
        self.borrow_entries().as_slice()
    }
}
