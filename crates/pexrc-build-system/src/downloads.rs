// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::bail;
use sha2::{Digest, Sha256};

use crate::config::DownloadArchive;

enum ArchiveType {
    TarLzma,
    TarGzip,
    Zip,
}

impl TryFrom<&str> for ArchiveType {
    type Error = anyhow::Error;

    fn try_from(path: &str) -> anyhow::Result<Self> {
        let archive_type = if [".tar.gz", ".tgz"].iter().any(|ext| path.ends_with(ext)) {
            ArchiveType::TarGzip
        } else if [".tar.xz", ".tar.lzma", ".tlz"]
            .iter()
            .any(|ext| path.ends_with(ext))
        {
            ArchiveType::TarLzma
        } else if [".zip"].iter().any(|ext| path.ends_with(ext)) {
            ArchiveType::Zip
        } else {
            bail!("No support for downloading archives of this sort: {path}");
        };
        Ok(archive_type)
    }
}

struct DigestReader<'a, D: Digest, R: Read> {
    digest: D,
    reader: R,
    source: &'a str,
    expected_size: u64,
    amount_read: u64,
}

impl<'a, D: Digest, R: Read> DigestReader<'a, D, R> {
    fn new(expected_size: u64, digest: D, reader: R, source: &'a str) -> Self {
        Self {
            digest,
            reader,
            source,
            expected_size,
            amount_read: 0,
        }
    }

    fn check(self, expected_size: u64, expected_hash: &str, source: &str) -> anyhow::Result<()> {
        if self.amount_read != expected_size {
            bail!(
                "Size of {source} was expected to be {expected_size} bytes but was actually \
                {actual_size} bytes.",
                actual_size = self.amount_read
            );
        }
        let actual_hash = hex::encode(self.digest.finalize().as_slice());
        if actual_hash != expected_hash {
            bail!(
                "Fingerprint of {source} did not match:\n\
                Expected: {expected_hash}\n\
                Actual:   {actual_hash}"
            );
        }
        Ok(())
    }
}

impl<'a, D: Digest, R: Read> Read for DigestReader<'a, D, R> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, io::Error> {
        let amount_read = self.reader.read(buffer)?;
        self.amount_read +=
            u64::try_from(amount_read).expect("The pointer size will not be greater than 64 bits.");
        if self.amount_read > self.expected_size {
            return Err(io::Error::new(
                ErrorKind::FileTooLarge,
                format!(
                    "Read {total_read} bytes from {source} but it was expected to be \
                    {expected_size} bytes.",
                    total_read = self.amount_read,
                    source = self.source,
                    expected_size = self.expected_size
                ),
            ));
        }
        self.digest.update(&buffer[0..amount_read]);
        Ok(amount_read)
    }
}

pub(crate) fn ensure_download(
    download: &DownloadArchive,
    download_dir: &Path,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(download_dir)?;
    let dst_dir = download_dir.join(download.fingerprint.hash);
    let downloaded_path = Ok(if let Some(prefix) = download.prefix {
        dst_dir.join(prefix)
    } else {
        dst_dir.clone()
    });

    // Double-checked lock.
    if dst_dir.exists() {
        return downloaded_path;
    }
    let lock_file = File::create(dst_dir.with_added_extension("lck"))?;
    lock_file.lock()?;
    if dst_dir.exists() {
        return downloaded_path;
    }

    let hasher = match download.fingerprint.algorithm {
        "sha256" => Sha256::new(),
        algorithm => bail!("No support for {algorithm} hashes."),
    };

    let url = reqwest::Url::parse(download.url.as_ref())?;
    let archive_type = ArchiveType::try_from(url.path())?;

    let response = reqwest::blocking::get(url)?;
    if let Some(actual_size) = response.content_length()
        && actual_size != download.size
    {
        bail!(
            "Expected {url} to be {expected_size} bytes but is {actual_size} bytes.",
            url = download.url,
            expected_size = download.size
        );
    }
    let download_dir = tempfile::TempDir::new_in(download_dir)?;
    let mut digest_reader =
        DigestReader::new(download.size, hasher, response, download.url.as_ref());
    match archive_type {
        ArchiveType::TarGzip => {
            let mut tar_stream =
                tar::Archive::new(flate2::read::GzDecoder::new(&mut digest_reader));
            tar_stream.unpack(download_dir.path())?;
        }
        ArchiveType::TarLzma => {
            let mut tar_stream = tar::Archive::new(xz2::read::XzDecoder::new(&mut digest_reader));
            tar_stream.unpack(download_dir.path())?;
        }
        ArchiveType::Zip => {
            let mut tmp = tempfile::tempfile_in(download_dir.path())?;
            io::copy(&mut digest_reader, &mut tmp)?;
            let mut zip = zip::ZipArchive::new(&mut tmp)?;
            zip.extract(download_dir.path())?;
        }
    }
    digest_reader.check(
        download.size,
        download.fingerprint.hash,
        download.url.as_ref(),
    )?;
    fs::rename(download_dir.keep(), dst_dir)?;
    downloaded_path
}
