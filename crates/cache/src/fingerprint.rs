// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io;
use std::path::Path;

use base64::Engine;
use base64::display::Base64Display;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use logging_timer::time;
use sha2::{Digest, Sha256};

pub struct Fingerprint(Vec<u8>);

impl Fingerprint {
    #[time("debug", "Fingerprint.{}")]
    pub fn base64_digest(&self) -> String {
        URL_SAFE_NO_PAD.encode(&self.0)
    }
}

impl Display for Fingerprint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Base64Display::new(&self.0, &URL_SAFE_NO_PAD).fmt(f)
    }
}

#[time("debug", "fingerprint.{}")]
pub fn hash_file(path: &Path, hash_path: bool) -> anyhow::Result<Fingerprint> {
    let mut digest = Sha256::new();
    if hash_path {
        digest.update(b"path:");
        digest.update(path.as_os_str().as_encoded_bytes());
    }
    digest.update(b"contents:");
    io::copy(&mut File::open(path)?, &mut digest)?;
    Ok(Fingerprint(Vec::from(digest.finalize().as_slice())))
}
