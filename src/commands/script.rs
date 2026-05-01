// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use fs_err as fs;
use python_proxy::ProxySource;
use target::SimplifiedTarget;

use crate::embeds::read_proxy_content;

pub fn create(
    target: SimplifiedTarget,
    python: &Path,
    script: &Path,
    output_file: &Path,
) -> anyhow::Result<()> {
    let proxy_bytes = Box::new(read_proxy_content(target)?);
    let script = fs::read_to_string(script)?;
    let target_script = fs::File::create(output_file)?;
    python_proxy::create(
        ProxySource::Read(proxy_bytes),
        python,
        target_script.into_file(),
        Some(script),
    )
}
