// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::PathBuf;

use crate::{Pex, PexInfo};

pub struct PexPath(Vec<PathBuf>);

impl PexPath {
    pub fn from_pex_info(pex_info: &PexInfo, allow_env_override: bool) -> Self {
        let mut pex_path = Vec::with_capacity(pex_info.pex_paths.len());
        if allow_env_override && let Some(path) = env::var_os("PEX_PATH") {
            pex_path.extend(env::split_paths(&path))
        } else if !pex_info.pex_paths.is_empty() {
            pex_path.extend_from_slice(&pex_info.pex_paths)
        } else if let Some(legacy_pex_path) = pex_info.pex_path.as_deref() && !legacy_pex_path.is_empty() {
            // Legacy PEX-INFO stored this in a single string as a colon-separated list.
            for entry in legacy_pex_path.split(':') {
                pex_path.push(entry.into())
            }
        }
        Self(pex_path)
    }

    pub fn load_pexes(&self) -> anyhow::Result<Vec<Pex<'_>>> {
        let mut pexes = Vec::with_capacity(self.0.len());
        for path in &self.0 {
            pexes.push(Pex::load(path)?)
        }
        Ok(pexes)
    }
}
