// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]
#![feature(str_as_str)]

mod dependency_configuration;
mod pex;
mod pex_info;
mod pex_path;
mod wheel;

pub use dependency_configuration::DependencyConfiguration;
pub use pex::{
    CollectWheelMetadata,
    DataDir,
    DistInfoDir,
    Layout,
    Pex,
    PexInfoDir,
    Resolve,
    ResolveError,
    ResolvedWheel,
    ResolvedWheels,
    collect_loose_user_source,
    collect_zipped_user_source_indexes,
    filter_zipped_user_source,
};
pub use pex_info::{BinPath, InheritPath, InterpreterSelectionStrategy, PexInfo, RawPexInfo};
pub use pex_path::PexPath;
pub use wheel::{
    EntryPoint,
    EntryPoints,
    Record,
    WheelFile,
    WheelLayout,
    WheelMetadata,
    WheelOptions,
    recompress_zipped_whl,
    repackage_wheels,
};
