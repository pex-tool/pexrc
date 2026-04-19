// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(trim_prefix_suffix)]

mod provenance;
pub mod venv_pex;
pub mod virtualenv;

pub use provenance::{Collision, CollisionReport, Provenance};
pub use venv_pex::{InstallScope, populate, populate_user_code_and_wheels};
pub use virtualenv::{Linker, Virtualenv};
