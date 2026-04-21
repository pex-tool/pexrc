// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::cmp;
use std::io::BufReader;
use std::path::Path;

use cache::Fingerprint;
use owo_colors::OwoColorize;
use pex::Pex;
use zip::CompressionMethod;

pub fn to_dir(
    dest_dir: &Path,
    pex: Pex,
    compression_method: CompressionMethod,
    compression_level: Option<i64>,
) -> anyhow::Result<()> {
    let wheels = pex::repackage_wheels(&pex, compression_method, compression_level, dest_dir)?;
    let count = wheels.len();

    let mut wheel_info = Vec::with_capacity(count);
    let mut max_width = 0;
    for wheel in wheels {
        let path = wheel.path().display().to_string();
        max_width = cmp::max(max_width, path.len());
        wheel_info.push((
            path,
            wheel.metadata()?,
            Fingerprint::try_from(BufReader::new(wheel))?,
        ));
    }

    anstream::println!(
        "Extracted {count} {wheels}:",
        count = count.yellow(),
        wheels = if count == 1 { "wheel" } else { "wheels" }
    );
    for (idx, (path, metadata, fingerprint)) in wheel_info.into_iter().enumerate() {
        anstream::println!(
            "{idx:>3}. {path} {pad}{size:<8} bytes {alg}:{fingerprint}",
            idx = (idx + 1).yellow(),
            pad = " ".repeat(max_width - path.len()),
            size = metadata.len().yellow(),
            alg = "sha256-base64".green(),
            fingerprint = fingerprint.base64_digest().green(),
        )
    }
    Ok(())
}
