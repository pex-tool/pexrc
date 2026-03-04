// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail};
use indexmap::{IndexMap, IndexSet};
use interpreter::{Interpreter, InterpreterConstraints};
use itertools::Itertools;
use log::{Level, debug};
use logging_timer::{time, timer};
use pep440_rs::Version;
use pep508_rs::{ExtraName, PackageName, Requirement, VersionOrUrl};
use python::{InterpreterIdentificationScript, ResourcePath, Resources};
use rayon::prelude::*;
use url::Url;
use zip::ZipArchive;

use crate::PexInfo;
use crate::wheel::{MetadataReader, Tag, WheelFile, WheelMetadata};

pub trait WheelResolver {
    fn resolve(&self, interpreter: &Interpreter) -> anyhow::Result<IndexSet<&str>>;
}

pub struct LoosePex<'a>(pub &'a Path, pub PexInfo);
pub struct PackedPex<'a>(pub &'a Path, pub PexInfo);
pub struct ZipAppPex<'a>(pub &'a Path, pub PexInfo);

impl<'a> ZipAppPex<'a> {
    pub(crate) fn resources(&self) -> anyhow::Result<impl Resources<'a>> {
        ZipResources::new(self.0)
    }
}

struct ZipAppPexMetadataReader<'a> {
    zip: &'a mut ZipArchive<File>,
    wheel_file_name: &'a str,
}

impl<'a> MetadataReader for ZipAppPexMetadataReader<'a> {
    fn reader(&mut self, path_components: &[&str]) -> anyhow::Result<impl Read> {
        Ok(self.zip.by_name(
            &[".deps", self.wheel_file_name]
                .iter()
                .chain(path_components.iter())
                .join("/"),
        )?)
    }
}

// TODO: XXX: This just uses PEX-INFO to resolve wheel file names, it is not ZipAppPex-specific.
impl<'a> WheelResolver for ZipAppPex<'a> {
    #[time("debug", "WheelResolver.{}")]
    fn resolve(&self, interpreter: &Interpreter) -> anyhow::Result<IndexSet<&str>> {
        let python_version = Version::new([
            u64::from(interpreter.version.major),
            u64::from(interpreter.version.minor),
            u64::from(interpreter.version.micro),
        ]);

        let supported_tags: HashMap<Tag, usize> = interpreter
            .supported_tags
            .iter()
            .enumerate()
            .map(|(idx, tag)| Tag::parse(tag).map(|tag| (tag, idx)))
            .collect::<anyhow::Result<_>>()?;

        let wheel_files = self
            .1
            .parse_distributions()
            .collect::<Result<Vec<(&str, WheelFile)>, _>>()?;

        let wheel_files = wheel_files
            .into_iter()
            .filter_map(|(file_name, wheel_file)| {
                for tag in &wheel_file.tags {
                    if let Some(rank) = supported_tags.get(tag) {
                        return Some((file_name, wheel_file, *rank));
                    }
                }
                None
            })
            .collect::<Vec<_>>();

        let mut wheels = Vec::with_capacity(wheel_files.len());
        let mut zip = ZipArchive::new(File::open(self.0)?)?;
        for (file_name, wheel_file, rank) in wheel_files {
            let wheel = WheelMetadata::parse(
                wheel_file,
                ZipAppPexMetadataReader {
                    zip: &mut zip,
                    wheel_file_name: file_name,
                },
            )?;
            if let Some(requires_python) = &wheel.requires_python
                && !requires_python.contains(&python_version)
            {
                continue;
            }
            wheels.push((file_name, wheel, rank));
        }

        struct WheelInfo<'b>(&'b str, Version, Vec<Requirement<Url>>, usize);

        let mut wheels_by_project_name: HashMap<PackageName, Vec<WheelInfo>> =
            HashMap::with_capacity(wheels.len());
        for (file_name, wheel, rank) in wheels {
            wheels_by_project_name
                .entry(wheel.wheel_file.project_name)
                .or_default()
                .push(WheelInfo(
                    file_name,
                    wheel.wheel_file.version,
                    wheel.requires_dists,
                    rank,
                ))
        }
        for wheels in wheels_by_project_name.values_mut() {
            wheels.sort_by_key(|WheelInfo(_, _, _, rank)| *rank);
        }

        let mut resolved_by_project_name: IndexMap<PackageName, &str> =
            IndexMap::with_capacity(wheels_by_project_name.len());
        let mut indexed_extras: Vec<Vec<ExtraName>> = vec![Vec::new()];
        let mut to_resolve: VecDeque<(Requirement<Url>, usize)> = self
            .1
            .requirements
            .iter()
            .map(|requirement| {
                Requirement::from_str(requirement).map(|requirement| (requirement, 0))
            })
            .collect::<Result<_, _>>()?;
        while let Some((requirement, extras_index)) = to_resolve.pop_front() {
            if resolved_by_project_name.contains_key(&requirement.name) {
                continue;
            }
            if !requirement
                .marker
                .evaluate(&interpreter.marker_env, &indexed_extras[extras_index])
            {
                continue;
            }
            let wheels = wheels_by_project_name
                .remove(&requirement.name)
                .ok_or_else(|| {
                    let inapplicable_wheels = self
                        .1
                        .parse_distributions()
                        .filter_map(|result| match result {
                            Ok((file_name, wheel_file))
                                if wheel_file.project_name == requirement.name =>
                            {
                                Some(file_name)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let count = inapplicable_wheels.len();
                    let wheels = if count == 1 { "wheel" } else { "wheels" };
                    let reason = if inapplicable_wheels.is_empty() {
                        format_args!(
                            "The PEX contains {count} embedded {wheels} for project: {project}",
                            project = requirement.name
                        )
                    } else {
                        format_args!(
                            "The PEX contains {count} inapplicable {wheels} for project: \
                            {project}\n\
                            {inapplicable_wheels}",
                            project = requirement.name,
                            inapplicable_wheels = inapplicable_wheels.join("\n")
                        )
                    };
                    anyhow!(
                        "The PEX at {path} has requirement {requirement} that cannot be satisfied \
                        for the interpreter at {python_exe}.\n\
                        {reason}",
                        path = self.0.display(),
                        python_exe = interpreter.path.display(),
                        reason = reason,
                    )
                })?;
            for WheelInfo(file_name, version, requirements, _) in wheels {
                if let Some(version_or_url) = requirement.version_or_url.as_ref() {
                    match version_or_url {
                        VersionOrUrl::VersionSpecifier(version_specifier) => {
                            if !version_specifier.contains(&version) {
                                continue;
                            }
                        }
                        VersionOrUrl::Url(url) => bail!(
                            "A PEX should never contain an URL requirement.\
                            The PEX at {path} requires: {url}",
                            path = self.0.display()
                        ),
                    }
                }
                let extras_index = if requirement.extras.is_empty() {
                    0
                } else {
                    let idx = indexed_extras.len();
                    indexed_extras.push(requirement.extras);
                    idx
                };
                resolved_by_project_name.insert(requirement.name, file_name);
                for req in requirements {
                    to_resolve.push_back((req, extras_index))
                }
                break;
            }
        }
        Ok(resolved_by_project_name.into_values().collect())
    }
}

struct ZipResources<R> {
    zip: ZipArchive<R>,
}

impl ZipResources<BufReader<File>> {
    fn new(path: &Path) -> anyhow::Result<Self> {
        let zip = ZipArchive::new(BufReader::new(File::open(path)?))?;
        Ok(Self { zip })
    }
}

impl<'a, R: Read + Seek> Resources<'a> for ZipResources<R> {
    fn read(&mut self, path: ResourcePath) -> anyhow::Result<Cow<'a, str>> {
        // TODO: XXX: The entry name logic here is shared with pexrc - centralize.
        let entry = self
            .zip
            .by_name(format!("__pex__/.scripts/{script}", script = path.script_name()).as_str())?;
        Ok(Cow::Owned(io::read_to_string(entry)?))
    }
}

pub enum Pex<'a> {
    Loose(LoosePex<'a>),
    Packed(PackedPex<'a>),
    ZipApp(ZipAppPex<'a>),
}

impl<'a> Pex<'a> {
    pub fn resources(&self) -> anyhow::Result<impl Resources<'a>> {
        match self {
            Pex::Loose(_) => todo!("XXX: Implement loose PEX resource resolution."),
            Pex::Packed(_) => todo!("XXX: Implement packed PEX resource resolution."),
            Pex::ZipApp(zip_app) => zip_app.resources(),
        }
    }
}

impl<'a> Pex<'a> {
    #[time("debug", "Pex.{}")]
    pub fn load(path: &'a Path) -> anyhow::Result<Self> {
        if path.is_file() {
            let zip_fp = File::open(path)?;
            let mut zip = {
                let _timer = timer!(Level::Debug; "Open PEX zip", "{}", path.display());
                ZipArchive::new(BufReader::new(zip_fp))?
            };
            let pex_info =
                PexInfo::parse(zip.by_name("PEX-INFO")?, Some(|| Cow::Borrowed("PEX-INFO")))?;
            Ok(Pex::ZipApp(ZipAppPex(path, pex_info)))
        } else {
            let bootstrap = path.join(".bootstrap");
            if !bootstrap.exists() {
                bail!(
                    "There is no PEX at {path}: it contains no `.bootstrap`.",
                    path = path.display()
                )
            }
            let pex_info_path = path.join("PEX-INFO");
            let pex_info_fp = File::open(&pex_info_path)?;
            let pex_info = PexInfo::parse(pex_info_fp, Some(|| pex_info_path.to_string_lossy()))?;
            if bootstrap.is_dir() {
                Ok(Pex::Loose(LoosePex(path, pex_info)))
            } else {
                Ok(Pex::Packed(PackedPex(path, pex_info)))
            }
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Pex::Loose(pex) => pex.0,
            Pex::Packed(pex) => pex.0,
            Pex::ZipApp(pex) => pex.0,
        }
    }

    pub fn info(&self) -> &PexInfo {
        match self {
            Pex::Loose(pex) => &pex.1,
            Pex::Packed(pex) => &pex.1,
            Pex::ZipApp(pex) => &pex.1,
        }
    }

    #[time("debug", "Pex.{}")]
    pub fn resolve(
        &self,
        python_exe: Option<&Path>,
    ) -> anyhow::Result<(Interpreter, IndexSet<&str>, impl Resources<'_>)> {
        let zip_app_pex = match self {
            Pex::Loose(_) => todo!("XXX: Implement loose PEX wheel resolution."),
            Pex::Packed(_) => todo!("XXX: Implement packed PEX wheel resolution."),
            Pex::ZipApp(zip_app) => zip_app,
        };

        let mut resources = zip_app_pex.resources()?;
        let identification_script = InterpreterIdentificationScript::read(&mut resources)?;

        let pex_info = self.info();
        let interpreter_constraints =
            InterpreterConstraints::try_from(&pex_info.interpreter_constraints)?;
        let mut errors = Vec::new();
        if let Some(python_exe) = python_exe
            && let Ok(interpreter) = Interpreter::load(python_exe, &identification_script)
            && interpreter_constraints.contains(&interpreter)
        {
            match zip_app_pex.resolve(&interpreter) {
                Ok(selected_wheels) => return Ok((interpreter, selected_wheels, resources)),
                Err(err) => errors.push((interpreter, err)),
            }
        }

        let interpreters_to_try = interpreter_constraints
            .iter_possibly_compatible_python_exes(pex_info.interpreter_selection_strategy.into())
            .collect::<Vec<_>>();
        let resolve_results_iter = interpreters_to_try
            .into_par_iter()
            .filter_map(|python_exe| Interpreter::load(python_exe, &identification_script).ok())
            .filter(|interpreter| interpreter_constraints.contains(interpreter))
            .map(|interpreter| match zip_app_pex.resolve(&interpreter) {
                Ok(selected_wheels) => Ok((interpreter, selected_wheels)),
                Err(err) => Err((interpreter, err)),
            });

        let errors: Arc<Mutex<Vec<(Interpreter, anyhow::Error)>>> = Arc::new(Mutex::new(errors));
        if let Some((interpreter, selected_wheels)) =
            resolve_results_iter.find_map_first(|result| match result {
                Ok((interpreter, selected_wheels)) => Some((interpreter, selected_wheels)),
                Err((interpreter, resolve_err)) => {
                    if let Err(lock_err) = errors.lock().map(|mut errors| {
                        debug!(
                            "Failed to resolve for {python_exe}: {resolve_err}",
                            python_exe = interpreter.path.display()
                        );
                        errors.push((interpreter, resolve_err))
                    }) {
                        debug!("Failed to record resolve error due to lock poisoning: {lock_err}");
                    }
                    None
                }
            })
        {
            return Ok((interpreter, selected_wheels, resources));
        }

        let reqs = &self.info().requirements;
        let requirement_count = reqs.len();
        let requirements = if requirement_count == 1 {
            "requirement"
        } else {
            "requirements"
        };

        let errors = errors.lock().map_err(|err| {
            anyhow!(
                "Failed to resolve requirements for PEX {path} and resolve errors were obfuscated \
                by a poisoned lock: {err}",
                path = zip_app_pex.0.display()
            )
        })?;
        let error_count = errors.len();
        let interpreters = if error_count == 1 {
            "interpreter"
        } else {
            "interpreters"
        };

        bail!(
            "Failed to resolve dependencies of PEX {path}.\n\
            \n\
            There are {requirement_count} root {requirements}:\n\
            {reqs}\n\
            \n\
            Tried resolving using {error_count} {interpreters}:\n\
            {errors}",
            path = self.path().display(),
            reqs = reqs.iter().map(|req| format!("+ {req}")).join("\n"),
            errors = errors
                .iter()
                .enumerate()
                .map(|(idx, (interpreter, err))| format!(
                    "{idx:>2} {path}: {err}",
                    idx = idx + 1,
                    path = interpreter.path.display()
                ))
                .join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::str::FromStr;

    use ::interpreter::Interpreter;
    use indexmap::IndexSet;
    use pep508_rs::{Requirement, VersionOrUrl};
    use python::InterpreterIdentificationScript;
    use rstest::{fixture, rstest};
    use testing::{interpreter_identification_script, python_exe, tmp_dir};
    use url::Url;

    use crate::wheel::WheelFile;
    use crate::{Pex, WheelResolver};

    #[fixture]
    fn pex(tmp_dir: PathBuf) -> PathBuf {
        let pex = tmp_dir.join("pex");
        Command::new("uvx")
            .args(["pex", "requests[socks]==2.32.5", "-o"])
            .arg(&pex)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        pex
    }

    #[rstest]
    fn test_resolve(
        pex: PathBuf,
        python_exe: &Path,
        interpreter_identification_script: InterpreterIdentificationScript,
    ) {
        let pex = match Pex::load(&pex).unwrap() {
            Pex::ZipApp(zip_app_pex) => zip_app_pex,
            _ => panic!("Unexpected pex type"),
        };
        let interpreter =
            Interpreter::load(python_exe, &interpreter_identification_script).unwrap();
        let resolved = pex
            .resolve(&interpreter)
            .unwrap()
            .into_iter()
            .map(|wheel_file_name| {
                WheelFile::parse_file_name(wheel_file_name)
                    .map(|wheel_file| (wheel_file.project_name, wheel_file.version))
            })
            .collect::<Result<IndexSet<_>, _>>()
            .unwrap();

        let expected_requirements: Vec<Requirement<Url>> = vec![
            Requirement::from_str("requests[socks]==2.32.5").unwrap(),
            Requirement::from_str("charset_normalizer<4,>=2").unwrap(),
            Requirement::from_str("idna<4,>=2.5").unwrap(),
            Requirement::from_str("urllib3<3,>=1.21.1").unwrap(),
            Requirement::from_str("certifi>=2017.4.17").unwrap(),
            Requirement::from_str("PySocks!=1.5.7,>=1.5.6; extra == \"socks\"").unwrap(),
        ];
        for (expected_requirement, (project_name, version)) in
            itertools::zip_eq(expected_requirements, resolved)
        {
            assert_eq!(expected_requirement.name, project_name);
            let version_specifier = match expected_requirement.version_or_url {
                Some(VersionOrUrl::VersionSpecifier(version_specifier)) => version_specifier,
                _ => panic!("Expected all requirements have version specifiers."),
            };
            assert!(version_specifier.contains(&version));
        }
    }
}
