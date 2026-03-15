// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::bail;
use indexmap::{IndexSet, indexset};
use log::{debug, warn};
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{ExtraName, MarkerTree, PackageName, Requirement, VersionOrUrl};
use time::Month;
use url::Url;
use which::sys::{RealSys, Sys};
use which::which_in_global;

use crate::{Interpreter, SearchPath};

#[derive(Debug, Hash, Eq, PartialEq)]
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
        if name.as_ref() == "pypy" && extras.is_empty() {
            return Ok(Self::PyPy);
        } else if name.as_ref() == "cpython" {
            if extras.is_empty() {
                return Ok(Self::CPython);
            } else if extras.len() == 1 && extras[0].as_ref() == "free-threaded" {
                return Ok(Self::CPythonFreeThreaded);
            } else if extras.len() == 1 && extras[0].as_ref() == "gil" {
                return Ok(Self::CPythonGil);
            }
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

#[derive(Debug, Eq, PartialEq)]
struct InterpreterConstraint {
    implementation: Option<InterpreterImplementation>,
    version_specifiers: Option<VersionSpecifiers>,
}

impl InterpreterConstraint {
    const ANY: Self = Self {
        implementation: None,
        version_specifiers: None,
    };

    fn parse(constraint: &str) -> anyhow::Result<Self> {
        if let Ok(version_specifiers) = VersionSpecifiers::from_str(constraint) {
            return Ok(Self {
                implementation: None,
                version_specifiers: Some(version_specifiers),
            });
        }

        for (prefix, implementation) in [
            ("CPython+t", InterpreterImplementation::CPythonFreeThreaded),
            ("CPython-t", InterpreterImplementation::CPythonGil),
        ] {
            if let Some(suffix) = constraint.strip_prefix(prefix) {
                let version_specifiers = if suffix.is_empty() {
                    None
                } else {
                    Some(VersionSpecifiers::from_str(suffix)?)
                };
                return Ok(Self {
                    implementation: Some(implementation),
                    version_specifiers,
                });
            }
        }

        let requirement: Requirement<Url> = Requirement::from_str(constraint)?;
        if requirement.marker != MarkerTree::default() {
            bail!(
                "Marker expressions are not supported in interpreter constraints; \
                given: {constraint}"
            );
        }

        let implementation =
            InterpreterImplementation::parse(&requirement.name, &requirement.extras, constraint)?;
        if let Some(version_or_url) = requirement.version_or_url {
            match version_or_url {
                VersionOrUrl::Url(_url) => bail!(
                    "Direct reference URLs are not supported for interpreter constraints, \
                    version specifiers can be used to restrict interpreter versions instead; \
                    given: {constraint}"
                ),
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
        // N.B.: This computes the maximum CPython minor version assuming CPython sticks to ~semver
        // and does not switch to calver.
        // + Release Schedule: https://peps.python.org/pep-0602/
        // + Rejected calver proposal: https://peps.python.org/pep-2026/
        //
        // Given PyPy history and the structure of the project, this max should always be greater
        // than the PyPy max minor.
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
    let mut supported_versions = SUPPORTED_VERSIONS.clone();
    supported_versions.reverse();
    supported_versions
});

#[derive(Eq, PartialEq)]
pub enum SelectionStrategy {
    Oldest,
    Newest,
}

#[derive(Hash, Eq, PartialEq)]
struct PythonBinarySpec {
    name: &'static str,
    major: u8,
    minor: u8,
    suffix: Option<&'static str>,
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

    fn calculate_compatible_binary_specs(
        &self,
        selection_strategy: SelectionStrategy,
    ) -> IndexSet<PythonBinarySpec> {
        let versions = match selection_strategy {
            SelectionStrategy::Oldest => &SUPPORTED_VERSIONS,
            SelectionStrategy::Newest => &SUPPORTED_VERSIONS_NEWEST_FIRST,
        };
        let constraints = if self.0.is_empty() {
            &[InterpreterConstraint::ANY]
        } else {
            self.0.as_slice()
        };
        let mut binary_specs: IndexSet<PythonBinarySpec> = IndexSet::new();
        for (major, minor) in versions.iter() {
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
        binary_specs
    }

    pub fn calculate_compatible_binary_names(
        &self,
        selection_strategy: SelectionStrategy,
    ) -> IndexSet<OsString> {
        let binary_specs = self.calculate_compatible_binary_specs(selection_strategy);
        let mut binary_names: IndexSet<OsString> = IndexSet::new();
        for binary_spec in &binary_specs {
            binary_names.insert(
                format!(
                    "{name}{major}.{minor}{suffix}",
                    name = binary_spec.name,
                    major = binary_spec.major,
                    minor = binary_spec.minor,
                    suffix = binary_spec.suffix.unwrap_or("")
                )
                .into(),
            );
        }
        for binary_spec in &binary_specs {
            binary_names.insert(
                format!(
                    "{name}{major}",
                    name = binary_spec.name,
                    major = binary_spec.major
                )
                .into(),
            );
        }
        for binary_spec in &binary_specs {
            binary_names.insert(binary_spec.name.into());
        }
        binary_names
    }

    pub fn iter_possibly_compatible_python_exes(
        &self,
        selection_strategy: SelectionStrategy,
        search_path: SearchPath,
    ) -> anyhow::Result<impl Iterator<Item = PathBuf>> {
        let (python, path, known_paths) = search_path.into_parts()?;
        let binary_names = if let Some(python) = python {
            indexset! {python}
        } else {
            self.calculate_compatible_binary_names(selection_strategy)
        };
        Ok(PythonExeIter {
            known_paths,
            path,
            binary_names: binary_names.into_iter(),
            which_fn: which_in_global,
            binary_paths: None,
            seen: HashSet::new(),
        })
    }
}

struct PythonExeIter<
    KnownBinaryPaths: Iterator<Item = PathBuf>,
    Name,
    BinaryNames: Iterator<Item = Name>,
    BinaryPaths: Iterator<Item = PathBuf>,
    WhichError,
    WhichFunction: Fn(Name, Option<OsString>) -> Result<BinaryPaths, WhichError>,
> {
    known_paths: Option<KnownBinaryPaths>,
    path: Option<OsString>,
    binary_names: BinaryNames,
    which_fn: WhichFunction,
    binary_paths: Option<BinaryPaths>,
    seen: HashSet<PathBuf>,
}

impl<
    KnownBinaryPaths: Iterator<Item = PathBuf>,
    BinaryNames: Iterator<Item = OsString>,
    BinaryPaths: Iterator<Item = PathBuf>,
    WhichError,
    WhichFunction: Fn(OsString, Option<OsString>) -> Result<BinaryPaths, WhichError>,
> Iterator
    for PythonExeIter<
        KnownBinaryPaths,
        OsString,
        BinaryNames,
        BinaryPaths,
        WhichError,
        WhichFunction,
    >
{
    type Item = PathBuf;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(known_paths) = self.known_paths.as_mut() {
                if let Some(binary_path) = known_paths.next() {
                    if let Ok(real_binary_path) = binary_path.canonicalize() {
                        if self.seen.contains(real_binary_path.as_path()) {
                            continue;
                        }
                        self.seen.insert(real_binary_path.clone());
                        return Some(real_binary_path);
                    } else {
                        // E.G: A broken symbolic link.
                        continue;
                    }
                } else {
                    self.known_paths = None;
                }
            } else if let Some(binary_paths) = self.binary_paths.as_mut() {
                if let Some(binary_path) = binary_paths.next() {
                    if let Ok(real_binary_path) = binary_path.canonicalize() {
                        if self.seen.contains(real_binary_path.as_path()) {
                            continue;
                        }
                        self.seen.insert(real_binary_path.clone());
                        return Some(real_binary_path);
                    } else {
                        // E.G: A broken symbolic link.
                        continue;
                    }
                } else {
                    self.binary_paths = None;
                }
            } else if let Some(binary_name) = self.binary_names.next()
                && let Ok(binary_paths) = (self.which_fn)(
                    binary_name,
                    self.path.clone().or_else(|| RealSys.env_path()),
                )
            {
                self.binary_paths = Some(binary_paths);
            } else {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::str::FromStr;

    use pep440_rs::VersionSpecifiers;

    use crate::constraints::{InterpreterConstraint, InterpreterImplementation};
    use crate::{InterpreterConstraints, SelectionStrategy};

    #[test]
    fn test_parse_interpreter_constraint() {
        assert_eq!(
            InterpreterConstraint {
                implementation: None,
                version_specifiers: Some(VersionSpecifiers::from_str(">=3.14").unwrap())
            },
            InterpreterConstraint::parse(">=3.14").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPython),
                version_specifiers: None
            },
            InterpreterConstraint::parse("CPython").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPythonFreeThreaded),
                version_specifiers: None
            },
            InterpreterConstraint::parse("CPython+t").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPythonFreeThreaded),
                version_specifiers: Some(VersionSpecifiers::from_str("==3.15.*").unwrap())
            },
            InterpreterConstraint::parse("CPython+t==3.15.*").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPythonGil),
                version_specifiers: None
            },
            InterpreterConstraint::parse("CPython-t").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPythonGil),
                version_specifiers: Some(VersionSpecifiers::from_str("==3.13.*").unwrap())
            },
            InterpreterConstraint::parse("CPython-t==3.13.*").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPythonFreeThreaded),
                version_specifiers: None
            },
            InterpreterConstraint::parse("CPython[free-threaded]").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::CPythonGil),
                version_specifiers: None
            },
            InterpreterConstraint::parse("CPython[gil]").unwrap()
        );
        assert_eq!(
            InterpreterConstraint {
                implementation: Some(InterpreterImplementation::PyPy),
                version_specifiers: None
            },
            InterpreterConstraint::parse("PyPy").unwrap()
        );
    }

    fn os_str(value: &str) -> &OsStr {
        // SAFETY: Tests use ascii.
        unsafe { OsStr::from_encoded_bytes_unchecked(value.as_bytes()) }
    }

    #[test]
    fn test_interpreter_constraints_binary_names_all_default_order() {
        let ics = InterpreterConstraints::try_from::<&str>(&[]).unwrap();
        let binary_names = ics.calculate_compatible_binary_names(SelectionStrategy::Oldest);

        assert_eq!(&["python2.7", "pypy2.7"], &binary_names[0..2]);

        assert!(
            binary_names.get_index_of(os_str("pypy2.7"))
                < binary_names.get_index_of(os_str("python3.14"))
        );
        assert!(
            binary_names.get_index_of(os_str("python3.14"))
                < binary_names.get_index_of(os_str("pypy3.14"))
        );
        assert!(
            binary_names.get_index_of(os_str("pypy3.14"))
                < binary_names.get_index_of(os_str("python3.15"))
        );
        assert!(
            binary_names.get_index_of(os_str("python3.15"))
                < binary_names.get_index_of(os_str("pypy3.15"))
        );
        assert_eq!(
            &["python2", "pypy2", "python3", "pypy3", "python", "pypy"],
            &binary_names[binary_names.len() - 6..]
        );

        assert!(!binary_names.contains(os_str("python2.6")));
        assert!(!binary_names.contains(os_str("python2.8")));
        assert!(!binary_names.contains(os_str("python3.0")));
        assert!(!binary_names.contains(os_str("python3.1")));
        assert!(!binary_names.contains(os_str("python3.2")));
        assert!(!binary_names.contains(os_str("python3.3")));
        assert!(!binary_names.contains(os_str("python3.4")));
        assert!(binary_names.contains(os_str("python3.5")));
        assert!(binary_names.contains(os_str("python3.6")));

        assert!(!binary_names.contains(os_str("python3.12t")));
        assert!(binary_names.contains(os_str("python3.13t")));
        assert!(binary_names.contains(os_str("python3.14t")));
        assert!(binary_names.contains(os_str("python3.15t")));
    }

    #[test]
    fn test_interpreter_constraints_binary_names_all_newest_first() {
        let ics = InterpreterConstraints::try_from::<&str>(&[]).unwrap();
        let binary_names = ics.calculate_compatible_binary_names(SelectionStrategy::Newest);

        assert!(
            binary_names.get_index_of(os_str("python3.15"))
                < binary_names.get_index_of(os_str("pypy3.15"))
        );
        assert!(
            binary_names.get_index_of(os_str("pypy3.15"))
                < binary_names.get_index_of(os_str("python3.14"))
        );
        assert!(
            binary_names.get_index_of(os_str("python3.14"))
                < binary_names.get_index_of(os_str("pypy3.14"))
        );
        assert!(
            binary_names.get_index_of(os_str("pypy3.14"))
                < binary_names.get_index_of(os_str("python2.7"))
        );
        assert_eq!(
            &[
                "python2.7",
                "pypy2.7",
                "python3",
                "pypy3",
                "python2",
                "pypy2",
                "python",
                "pypy"
            ],
            &binary_names[binary_names.len() - 8..]
        );
    }

    #[test]
    fn test_interpreter_constraints_complex() {
        let ics = InterpreterConstraints::try_from::<&str>(&[
            "CPython+t==3.15.*",
            "CPython[free-threaded]==3.14.*",
            "CPython-t==3.13.*",
            "CPython[gil]==3.12.*",
            "PyPy>=3.9,<3.12",
        ])
        .unwrap();

        assert_eq!(
            &[
                "python3.15t",
                "python3.14t",
                "python3.13",
                "python3.12",
                "pypy3.11",
                "pypy3.10",
                "pypy3.9",
                "python3",
                "pypy3",
                "python",
                "pypy",
            ],
            ics.calculate_compatible_binary_names(SelectionStrategy::Newest)
                .as_slice()
        );
        assert_eq!(
            &[
                "pypy3.9",
                "pypy3.10",
                "pypy3.11",
                "python3.12",
                "python3.13",
                "python3.14t",
                "python3.15t",
                "pypy3",
                "python3",
                "pypy",
                "python",
            ],
            ics.calculate_compatible_binary_names(SelectionStrategy::Oldest)
                .as_slice()
        );
    }
}
