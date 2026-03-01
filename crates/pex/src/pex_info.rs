// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::io::Read;

use anyhow::anyhow;
use interpreter::SelectionStrategy;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum BinPath {
    #[serde(rename = "false")]
    False,
    #[serde(rename = "append")]
    Append,
    #[serde(rename = "prepend")]
    Prepend,
}

impl BinPath {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinPath::False => "false",
            BinPath::Append => "append",
            BinPath::Prepend => "prepend",
        }
    }
}

#[derive(Debug, Deserialize)]
pub enum InheritPath {
    #[serde(rename = "false")]
    False,
    #[serde(rename = "prefer")]
    Prefer,
    #[serde(rename = "fallback")]
    Fallback,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum InterpreterSelectionStrategy {
    #[serde(rename = "oldest")]
    Oldest,
    #[serde(rename = "newest")]
    Newest,
}

impl From<InterpreterSelectionStrategy> for SelectionStrategy {
    fn from(value: InterpreterSelectionStrategy) -> Self {
        match value {
            InterpreterSelectionStrategy::Oldest => SelectionStrategy::Oldest,
            InterpreterSelectionStrategy::Newest => SelectionStrategy::Newest,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PexInfo {
    pub bind_resource_paths: HashMap<String, String>,
    pub build_properties: HashMap<String, String>,
    pub deps_are_wheel_files: bool,
    pub distributions: HashMap<String, String>,
    pub entry_point: Option<String>,
    pub excluded: Vec<String>,
    pub inherit_path: Option<InheritPath>,
    pub inject_args: Vec<String>,
    pub inject_env: HashMap<String, String>,
    pub inject_python_args: Vec<String>,
    pub interpreter_constraints: Vec<String>,
    pub interpreter_selection_strategy: InterpreterSelectionStrategy,
    pub overridden: Vec<String>,
    pub pex_hash: String,
    pub pex_path: String,
    pub pex_paths: Vec<String>,
    pub pex_root: Option<String>,
    pub requirements: Vec<String>,
    pub script: Option<String>,
    pub strip_pex_env: Option<bool>,
    pub venv_bin_path: Option<BinPath>,
    pub venv_hermetic_scripts: bool,
    pub venv_system_site_packages: bool,
}

impl PexInfo {
    pub fn parse<'a>(
        data: impl Read,
        source: Option<impl FnOnce() -> Cow<'a, str>>,
    ) -> anyhow::Result<PexInfo> {
        let contents = io::read_to_string(data)?;
        serde_json::from_str(&contents).map_err(|err| {
            anyhow!(
                "Failed to parse PEX-INFO from {source}: {err}",
                source = source.map(|f| f()).unwrap_or(Cow::Borrowed("<string>"))
            )
        })
    }
}
