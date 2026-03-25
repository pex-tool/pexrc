// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter};
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use base64::Engine;
use base64::display::Base64Display;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fs_err::File;
use logging_timer::time;
use sha2::{Digest, Sha256};

pub struct Fingerprint(Vec<u8>);

impl Fingerprint {
    pub fn new<D: Digest>(digest: D) -> Self {
        Self(Vec::from(digest.finalize().as_slice()))
    }

    #[time("debug", "Fingerprint.{}")]
    pub fn base64_digest(&self) -> String {
        URL_SAFE_NO_PAD.encode(&self.0)
    }

    #[time("debug", "Fingerprint.{}")]
    pub fn hex_digest(&self) -> String {
        hex::encode(&self.0)
    }
}

impl Display for Fingerprint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Base64Display::new(&self.0, &URL_SAFE_NO_PAD).fmt(f)
    }
}

#[derive(Default)]
pub struct HashOptions {
    path: bool,
    mtime: bool,
    size: bool,
    contents: bool,
}

impl HashOptions {
    pub const fn new() -> Self {
        Self {
            path: false,
            mtime: false,
            size: false,
            contents: false,
        }
    }

    pub const fn path(mut self, path: bool) -> Self {
        self.path = path;
        self
    }

    pub const fn mtime(mut self, mtime: bool) -> Self {
        self.mtime = mtime;
        self
    }

    pub const fn size(mut self, size: bool) -> Self {
        self.size = size;
        self
    }

    pub const fn contents(mut self, contents: bool) -> Self {
        self.contents = contents;
        self
    }
}

#[time("debug", "fingerprint.{}")]
pub fn hash_file(path: &Path, options: &HashOptions) -> anyhow::Result<Fingerprint> {
    let mut digest = Sha256::new();
    digest_file(path, options, &mut digest)?;
    Ok(Fingerprint::new(digest))
}

pub(crate) fn digest_file<D>(
    path: &Path,
    options: &HashOptions,
    digest: &mut D,
) -> anyhow::Result<()>
where
    D: Digest + Write,
{
    if options.path {
        digest.update(b"path:");
        digest.update(path.as_os_str().as_encoded_bytes());
    }
    if options.mtime || options.size {
        let metadata = path.metadata()?;
        if options.mtime {
            digest.update(b"mtime:");
            digest.update(
                metadata
                    .modified()?
                    .duration_since(SystemTime::UNIX_EPOCH)?
                    .as_nanos()
                    .to_ne_bytes(),
            )
        }
        if options.size {
            digest.update(b"size:");
            digest.update(metadata.len().to_ne_bytes())
        }
    }
    if options.contents {
        digest.update(b"contents:");
        io::copy(&mut File::open(path)?, digest)?;
    }
    Ok(())
}
