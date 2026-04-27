// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

mod constraints;
mod interpreter;

#[cfg(target_os = "linux")]
mod linux;

mod platform;
mod search_path;
mod tag;

pub use constraints::{
    InterpreterConstraint,
    InterpreterConstraints,
    SelectionStrategy,
    VersionSpec,
};
pub use interpreter::Interpreter;
pub use platform::Platform;
pub use search_path::SearchPath;
pub use tag::Tag;
