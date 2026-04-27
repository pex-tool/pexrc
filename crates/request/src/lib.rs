// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use reqwest::IntoUrl;
use reqwest::blocking::Response;

#[cfg(target_os = "macos")]
static INSTALLED: std::sync::LazyLock<anyhow::Result<bool>> = std::sync::LazyLock::new(|| {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map(|_| true)
        .map_err(|err| anyhow::anyhow!("Failed to install ring crypto provider: {err:?}"))
});

#[cfg(target_os = "macos")]
fn install() -> anyhow::Result<()> {
    INSTALLED
        .as_ref()
        .map(|_| ())
        .map_err(|err| anyhow::anyhow!("{err}"))
}

pub fn get(url: impl IntoUrl) -> anyhow::Result<Response> {
    #[cfg(target_os = "macos")]
    install()?;
    Ok(reqwest::blocking::get(url)?)
}
