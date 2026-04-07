// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use python_proxy::ProxySource;
use target::Target;

use crate::embeds::get_proxy_content;

pub fn create(
    target: &Target,
    python: &Path,
    script: &Path,
    output_file: &Path,
) -> anyhow::Result<()> {
    let proxy_bytes = get_proxy_content(target)?;
    let script = fs::read_to_string(script)?;
    let target_script = fs::File::create(output_file)?;
    python_proxy::create(
        ProxySource::Bytes(&proxy_bytes),
        python,
        target_script,
        Some(script),
    )
}
