// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::cmp;
use std::io::Write;

use cache::Fingerprint;
use owo_colors::OwoColorize;
use sha2::{Digest, Sha256};

use crate::clibs::CLIBS_DIR;

pub fn display() -> anyhow::Result<()> {
    let mut paths = Vec::new();
    let mut max_width = 0;
    for clib in CLIBS_DIR.files() {
        let path = clib.path().display().to_string();
        max_width = cmp::max(max_width, path.len());
        paths.push(path);
    }
    let count = paths.len();
    anstream::println!(
        "There {are} {count} embedded {clibs}:",
        are = if count == 1 { "is" } else { "are" },
        count = count.yellow(),
        clibs = if count == 1 { "clib" } else { "clibs" }
    );
    for (idx, (clib, path)) in CLIBS_DIR.files().zip(paths).enumerate() {
        let mut digest = Sha256::new();
        digest.write_all(clib.contents())?;
        let fingerprint = Fingerprint::new(digest);
        anstream::println!(
            "{idx:>3}. {path} {pad}{size:<8} bytes {alg}:{fingerprint}",
            idx = (idx + 1).yellow(),
            path = path.blue(),
            pad = " ".repeat(max_width - path.len()),
            size = clib.contents().len().yellow(),
            alg = "sha256-base64".green(),
            fingerprint = fingerprint.base64_digest().green(),
        )
    }
    Ok(())
}
