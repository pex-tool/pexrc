// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;

use crate::{ResourcePath, Resources};

pub struct EmbeddedResources(());

impl Resources<'static> for EmbeddedResources {
    fn read(&mut self, path: ResourcePath) -> anyhow::Result<Cow<'static, str>> {
        Ok(Cow::Borrowed(match path {
            ResourcePath::BootScript => include_str!("boot.py"),
            ResourcePath::InterpreterIdentificationScript => include_str!("interpreter.py"),
            ResourcePath::VendoredVirtualenvScript => include_str!(env!("VIRTUALENV_PY")),
            ResourcePath::VenvPexScript => include_str!("venv-pex.py"),
            ResourcePath::VenvPexReplScript => include_str!("venv-pex-repl.py"),
        }))
    }
}

pub const RESOURCES: EmbeddedResources = EmbeddedResources(());
