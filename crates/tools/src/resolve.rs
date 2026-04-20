// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use indexmap::IndexMap;
use interpreter::{Interpreter, SearchPath};
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement};
use pex::{CollectExtraMetadata, Pex};
use url::Url;

pub(crate) struct WheeInfo<'a> {
    pub file_name: &'a str,
    pub raw_project_name: &'a str,
    pub raw_version: &'a str,
    pub version: Version,
    pub requires_dists: Vec<Requirement<Url>>,
    pub requires_python: Option<VersionSpecifiers>,
}

pub(crate) fn resolve<'a>(
    python: &Path,
    pex: &'a Pex<'a>,
    additional_pexes: &'a [Pex<'a>],
) -> anyhow::Result<(Interpreter, IndexMap<PackageName, WheeInfo<'a>>)> {
    let search_path = SearchPath::from_env()?;
    let extra_metadata = CollectExtraMetadata::new();
    let resolve = pex.resolve(
        Some(python),
        additional_pexes.iter(),
        search_path,
        Some(extra_metadata.clone()),
    )?;
    let metadata_lookups = extra_metadata.into_lookups()?;

    let mut wheels: IndexMap<PackageName, WheeInfo<'a>> = IndexMap::new();
    for wheel in resolve.wheels.values().chain(
        resolve
            .additional_wheels
            .iter()
            .flat_map(|(_, additional_wheels)| additional_wheels.values()),
    ) {
        let wheel_metadata = metadata_lookups
            .for_whl(wheel)
            .expect("Each resolved wheel should be paired with metadata");
        wheels.insert(
            wheel_metadata.project_name.clone(),
            WheeInfo {
                file_name: wheel_metadata.file_name,
                raw_project_name: wheel_metadata.raw_project_name,
                raw_version: wheel_metadata.raw_version,
                version: wheel_metadata.version.clone(),
                requires_dists: wheel_metadata.requires_dists.clone(),
                requires_python: wheel_metadata.requires_python.clone(),
            },
        );
    }
    Ok((resolve.interpreter, wheels))
}
