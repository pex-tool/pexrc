// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::bail;
use indexmap::IndexSet;
use indexmap::set::IntoIter;
use itertools::Itertools;
use log::{debug, warn};
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{ExtraName, MarkerTree, PackageName, Requirement, VersionOrUrl};
use time::Month;
use url::Url;
use which::which_in_global;

use crate::Interpreter;

#[derive(Hash, Eq, PartialEq)]
enum InterpreterImplementation {
    CPython,
    CPythonFreeThreaded,
    CPythonGil,
    PyPy,
}

impl InterpreterImplementation {
    fn of(interpreter: &Interpreter) -> Option<Self> {
        match interpreter.marker_env.platform_python_implementation() {
            "PyPy" => Some(Self::PyPy),
            "CPython" => {
                if let Some(freethreaded) = interpreter.free_threaded {
                    if freethreaded {
                        Some(Self::CPythonFreeThreaded)
                    } else {
                        Some(Self::CPythonGil)
                    }
                } else {
                    Some(Self::CPython)
                }
            }
            _ => None,
        }
    }

    fn matches(&self, other: &InterpreterImplementation) -> bool {
        match self {
            InterpreterImplementation::CPython => matches!(
                other,
                InterpreterImplementation::CPython
                    | InterpreterImplementation::CPythonFreeThreaded
                    | InterpreterImplementation::CPythonGil
            ),
            InterpreterImplementation::CPythonFreeThreaded => {
                *other == InterpreterImplementation::CPythonFreeThreaded
            }
            InterpreterImplementation::CPythonGil => matches!(
                other,
                InterpreterImplementation::CPython | InterpreterImplementation::CPythonGil
            ),
            InterpreterImplementation::PyPy => *other == InterpreterImplementation::PyPy,
        }
    }
}

impl InterpreterImplementation {
    fn parse(name: &PackageName, extras: &[ExtraName], source: &str) -> anyhow::Result<Self> {
        if name.as_ref() == "PyPy" && extras.is_empty() {
            return Ok(Self::PyPy);
        } else if name.as_ref() == "CPython" {
            if extras.is_empty() {
                return Ok(Self::CPython);
            } else if extras.len() == 1 && extras[0].as_ref() == "free-threaded" {
                return Ok(Self::CPythonFreeThreaded);
            } else if extras.len() == 1 && extras[0].as_ref() == "gil" {
                return Ok(Self::CPythonGil);
            }
        } else if name.as_ref() == "CPython+t" && extras.is_empty() {
            return Ok(Self::CPythonFreeThreaded);
        } else if name.as_ref() == "CPython-t" && extras.is_empty() {
            return Ok(Self::CPythonGil);
        }
        bail!(
            "Invalid interpreter implementation in: {source}\n\
            Only the following are recognized:\n\
            + PyPy: any PyPy interpreter\n\
            + CPython: any CPython interpreter\n\
            + CPython+t or CPython[free-threaded]: a free-threaded CPython interpreter\n\
            + CPython-t or CPython[gil]: a traditional GIL-enabled CPython interpreter",
        )
    }
}

struct InterpreterConstraint {
    implementation: Option<InterpreterImplementation>,
    version_specifiers: Option<VersionSpecifiers>,
}

impl InterpreterConstraint {
    fn parse(constraint: &str) -> anyhow::Result<Self> {
        if let Ok(version_specifiers) = VersionSpecifiers::from_str(constraint) {
            return Ok(Self {
                implementation: None,
                version_specifiers: Some(version_specifiers),
            });
        }

        let requirement: Requirement<Url> = Requirement::from_str(constraint)?;
        if requirement.marker != MarkerTree::default() {
            bail!("000")
        }

        let implementation =
            InterpreterImplementation::parse(&requirement.name, &requirement.extras, constraint)?;
        if let Some(version_or_url) = requirement.version_or_url {
            match version_or_url {
                VersionOrUrl::Url(_url) => bail!("111"),
                VersionOrUrl::VersionSpecifier(version_specifiers) => Ok(Self {
                    implementation: Some(implementation),
                    version_specifiers: Some(version_specifiers),
                }),
            }
        } else {
            Ok(Self {
                implementation: Some(implementation),
                version_specifiers: None,
            })
        }
    }

    fn contains(&self, interpreter: &Interpreter) -> bool {
        if let Some(implementation) = self.implementation.as_ref() {
            if let Some(other_implementation) = InterpreterImplementation::of(interpreter) {
                if !implementation.matches(&other_implementation) {
                    return false;
                }
            } else {
                return false;
            }
        }
        self.contains_version(interpreter.version.major, interpreter.version.minor)
    }

    fn contains_version(&self, major: u8, minor: u8) -> bool {
        if let Some(version_specifiers) = self.version_specifiers.as_ref() {
            let version = Version::new([u64::from(major), u64::from(minor)]);
            return version_specifiers.contains(&version);
        }
        true
    }
}

impl Display for InterpreterConstraint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(implementation) = self.implementation.as_ref() {
            match implementation {
                InterpreterImplementation::CPython => f.write_str("CPython")?,
                InterpreterImplementation::CPythonFreeThreaded => {
                    f.write_str("CPython[free-threaded]")?
                }
                InterpreterImplementation::CPythonGil => f.write_str("CPython[gil]")?,
                InterpreterImplementation::PyPy => f.write_str("PyPy")?,
            }
        }
        if let Some(version_specifiers) = self.version_specifiers.as_ref() {
            f.write_fmt(format_args!("{}", version_specifiers))?;
        }
        Ok(())
    }
}

static SUPPORTED_VERSIONS: LazyLock<Vec<(u8, u8)>> = LazyLock::new(|| {
    let max_minor = {
        // N.B.: This computes the maximum CPython minor version assuming CPython sticks to ~semver and
        // does not switch to calver.
        // + Release Schedule: https://peps.python.org/pep-0602/
        // + Rejected calver proposal: https://peps.python.org/pep-2026/
        //
        // Given PyPy history and the structure of the project, this max should always be greater than
        // the PyPy max minor.
        //
        // The calibration point: 3.14.0 was released on 2025-10-07 and there are yearly releases.
        let today = time::UtcDateTime::now().date();
        let years_since_pi_release = today.year() - 2025;
        let max_minor = 14 + years_since_pi_release;
        let mut max_minor = u8::try_from(max_minor).unwrap_or_else(|err| {
            warn!(
            "Failed to guess the current production release of CPython using the baseline release \
            of 3.14 ion 2025-10-07.\n\
            At a yearly release cadence incrementing the minor version number, \
            {max_minor} has overflowed a u8: {err}\n\
            Continuing with assumed max CPython production release of 3.255"
        );
            u8::MAX
        });
        if today.month() < Month::October {
            max_minor -= 1;
        }
        // Give a 1-year buffer to account for testing the next release.
        max_minor + 1
    };
    [(2, 7)]
        .into_iter()
        .chain((5..=max_minor).map(|minor| (3, minor)))
        .collect()
});

static SUPPORTED_VERSIONS_NEWEST_FIRST: LazyLock<Vec<(u8, u8)>> = LazyLock::new(|| {
    SUPPORTED_VERSIONS
        .iter()
        .map(|(major, minor)| (*major, *minor))
        .sorted_by_key(|(major, minor)| (-i16::from(*major), -i16::from(*minor)))
        .collect::<Vec<_>>()
});

#[derive(Eq, PartialEq)]
pub enum SelectionStrategy {
    Oldest,
    Newest,
}

pub struct InterpreterConstraints(Vec<InterpreterConstraint>);

impl InterpreterConstraints {
    pub fn try_from<S: AsRef<str>>(constraints: &[S]) -> anyhow::Result<Self> {
        Ok(Self(
            constraints
                .iter()
                .map(|constraint| InterpreterConstraint::parse(constraint.as_ref()))
                .collect::<anyhow::Result<Vec<_>>>()?,
        ))
    }

    pub fn contains(&self, interpreter: &Interpreter) -> bool {
        self.0.is_empty()
            || self
                .0
                .iter()
                .any(|constraint| constraint.contains(interpreter))
    }

    pub fn iter_compatible_interpreters(
        &self,
        selection_strategy: SelectionStrategy,
    ) -> impl Iterator<Item = Interpreter> {
        // TODO: XXX: Account for PEX_PYTHON
        let search_path = env::var_os("PEX_PYTHON_PATH").or_else(|| env::var_os("PATH"));
        let versions = match selection_strategy {
            SelectionStrategy::Oldest => &SUPPORTED_VERSIONS,
            SelectionStrategy::Newest => &SUPPORTED_VERSIONS_NEWEST_FIRST,
        };
        InterpreterIter::new(self.0.as_slice(), search_path, versions.as_slice())
    }
}

#[derive(Hash, Eq, PartialEq)]
struct PythonBinarySpec {
    name: &'static str,
    major: u8,
    minor: u8,
    suffix: Option<&'static str>,
}

struct InterpreterIter<'a> {
    constraints: &'a [InterpreterConstraint],
    search_path: Option<OsString>,
    binary_names: IntoIter<String>,
    binary_paths: Option<Box<dyn Iterator<Item = PathBuf>>>,
}

impl<'a> InterpreterIter<'a> {
    fn new(
        constraints: &'a [InterpreterConstraint],
        search_path: Option<OsString>,
        versions: &'a [(u8, u8)],
    ) -> Self {
        let mut binary_specs: IndexSet<PythonBinarySpec> = IndexSet::new();
        for (major, minor) in versions {
            for constraint in constraints {
                if constraint.contains_version(*major, *minor) {
                    match constraint.implementation.as_ref() {
                        None => {
                            binary_specs.insert(PythonBinarySpec {
                                name: "python",
                                major: *major,
                                minor: *minor,
                                suffix: None,
                            });
                            if (*major, *minor) >= (3, 13) {
                                binary_specs.insert(PythonBinarySpec {
                                    name: "python",
                                    major: *major,
                                    minor: *minor,
                                    suffix: Some("t"),
                                });
                            }
                            binary_specs.insert(PythonBinarySpec {
                                name: "pypy",
                                major: *major,
                                minor: *minor,
                                suffix: None,
                            });
                        }
                        Some(implementation) => match implementation {
                            InterpreterImplementation::CPython
                            | InterpreterImplementation::CPythonGil => {
                                binary_specs.insert(PythonBinarySpec {
                                    name: "python",
                                    major: *major,
                                    minor: *minor,
                                    suffix: None,
                                });
                            }
                            InterpreterImplementation::CPythonFreeThreaded => {
                                if (*major, *minor) >= (3, 13) {
                                    binary_specs.insert(PythonBinarySpec {
                                        name: "python",
                                        major: *major,
                                        minor: *minor,
                                        suffix: Some("t"),
                                    });
                                } else {
                                    debug!(
                                        "Ignoring {constraint} for CPython {major}.{minor} since \
                                        free-threaded CPython only exists for >=3.13."
                                    );
                                }
                            }
                            InterpreterImplementation::PyPy => {
                                binary_specs.insert(PythonBinarySpec {
                                    name: "pypy",
                                    major: *major,
                                    minor: *minor,
                                    suffix: None,
                                });
                            }
                        },
                    }
                }
            }
        }
        let mut binary_names: IndexSet<String> = IndexSet::new();
        for binary_spec in &binary_specs {
            binary_names.insert(format!(
                "{name}{major}.{minor}{suffix}",
                name = binary_spec.name,
                major = binary_spec.major,
                minor = binary_spec.minor,
                suffix = binary_spec.suffix.unwrap_or("")
            ));
        }
        for binary_spec in &binary_specs {
            binary_names.insert(format!(
                "{name}{major}",
                name = binary_spec.name,
                major = binary_spec.major
            ));
        }
        for binary_spec in &binary_specs {
            binary_names.insert(binary_spec.name.to_string());
        }
        let binary_names_iter = binary_names.into_iter();
        Self {
            constraints,
            search_path,
            binary_names: binary_names_iter,
            binary_paths: None,
        }
    }
}

impl<'a> Iterator for InterpreterIter<'a> {
    type Item = Interpreter;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(binary_paths) = self.binary_paths.as_mut() {
                if let Some(binary_path) = binary_paths.next() {
                    match Interpreter::load(&binary_path) {
                        Ok(interpreter) => {
                            if self.constraints.is_empty()
                                || self
                                    .constraints
                                    .iter()
                                    .any(|constraint| constraint.contains(&interpreter))
                            {
                                return Some(interpreter);
                            }
                        }
                        Err(err) => {
                            debug!("Failed to load {path}: {err}", path = binary_path.display())
                        }
                    }
                } else {
                    self.binary_paths = None;
                }
            } else if let Some(binary_name) = self.binary_names.next()
                && let Ok(binary_paths) = which_in_global(binary_name, self.search_path.clone())
            {
                self.binary_paths = Some(Box::new(binary_paths));
            } else {
                return None;
            }
        }
    }
}
