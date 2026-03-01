// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::io::{Read, Seek};
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, bail};
use itertools::Itertools;
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement};
use python_pkginfo::Metadata;
use url::Url;
use zip::ZipArchive;

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct Tag<'a> {
    python: &'a str,
    abi: &'a str,
    platform: &'a str,
}

impl<'a> Tag<'a> {
    pub(crate) fn parse(tag: &'a str) -> anyhow::Result<Self> {
        let mut tags = tag.split("-");
        let python = tags.next().ok_or_else(|| anyhow!("333"))?;
        let abi = tags.next().ok_or_else(|| anyhow!("334"))?;
        let platform = tags.next().ok_or_else(|| anyhow!("335"))?;
        if tags.next().is_some() {
            bail!("336")
        }
        Ok(Self {
            python,
            abi,
            platform,
        })
    }
}

pub struct WheelFile<'a> {
    pub(crate) raw_project_name: &'a str,
    pub(crate) project_name: PackageName,
    pub(crate) raw_version: &'a str,
    pub(crate) version: Version,
    pub(crate) tags: Vec<Tag<'a>>,
}

impl<'a> WheelFile<'a> {
    pub fn parse(path: &'a Path) -> anyhow::Result<Self> {
        let file_name = path.file_name().and_then(OsStr::to_str).ok_or_else(|| {
            anyhow!(
                "Could not determine wheel filename from path: {path}",
                path = path.display()
            )
        })?;
        Self::parse_file_name(file_name)
    }

    pub(crate) fn parse_file_name(file_name: &'a str) -> anyhow::Result<Self> {
        // See: https://packaging.python.org/en/latest/specifications/binary-distribution-format/#file-name-convention
        // {distribution}-{version}(-{build tag})?-{python tag}-{abi tag}-{platform tag}.whl
        if !file_name.ends_with(".whl") {
            bail!("337")
        }

        let (raw_project_name, raw_version, python_tag, abi_tag, platform_tag) = {
            let mut trailing_components = file_name[0..file_name.len() - 4].rsplitn(4, "-");
            let platform_tag = trailing_components.next().ok_or_else(|| anyhow!("338"))?;
            let abi_tag = trailing_components.next().ok_or_else(|| anyhow!("339"))?;
            let python_tag = trailing_components.next().ok_or_else(|| anyhow!("340"))?;
            let rest = trailing_components.next().ok_or_else(|| anyhow!("341"))?;

            let mut leading_components = rest.splitn(2, "-");
            let project_name = leading_components.next().ok_or_else(|| anyhow!("342"))?;
            let version = leading_components.next().ok_or_else(|| anyhow!("343"))?;

            (project_name, version, python_tag, abi_tag, platform_tag)
        };

        let mut tags: Vec<Tag<'a>> = Vec::new();
        for python in python_tag.split(".") {
            for abi in abi_tag.split(".") {
                for platform in platform_tag.split(".") {
                    tags.push(Tag {
                        python,
                        abi,
                        platform,
                    })
                }
            }
        }

        let project_name = PackageName::from_str(raw_project_name)?;
        let version = Version::from_str(raw_version)?;
        Ok(Self {
            raw_project_name,
            project_name,
            raw_version,
            version,
            tags,
        })
    }
}

pub struct WheelMetadata<'a> {
    pub(crate) wheel_file: WheelFile<'a>,
    pub(crate) requires_dists: Vec<Requirement<Url>>,
    pub(crate) requires_python: Option<VersionSpecifiers>,
}

pub trait MetadataReader {
    fn reader(&mut self, path_components: &[&str]) -> anyhow::Result<impl Read>;
}

pub struct DirMetadataReader<'a>(&'a Path);

impl<'a> MetadataReader for DirMetadataReader<'a> {
    fn reader(&mut self, path_components: &[&str]) -> anyhow::Result<impl Read> {
        let mut read_path = self.0.to_owned();
        for component in path_components {
            read_path.push(component);
        }
        File::open(read_path).map_err(anyhow::Error::new)
    }
}

pub struct WhlMetadataReader<R>(ZipArchive<R>);

impl WhlMetadataReader<File> {
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self(ZipArchive::new(File::open(path.as_ref())?)?))
    }
}

impl<R: Read + Seek> MetadataReader for WhlMetadataReader<R> {
    fn reader(&mut self, path_components: &[&str]) -> anyhow::Result<impl Read> {
        self.0
            .by_name(&path_components.iter().join("/"))
            .map_err(anyhow::Error::new)
    }
}

impl<'a> WheelMetadata<'a> {
    pub fn try_from_path(path: &'a Path) -> anyhow::Result<Self> {
        let file_name = path.file_name().and_then(OsStr::to_str).ok_or_else(|| {
            anyhow!(
                "Could not determine wheel filename from path: {path}",
                path = path.display()
            )
        })?;
        let wheel_file = WheelFile::parse_file_name(file_name)?;
        if path.is_dir() {
            Self::parse(wheel_file, DirMetadataReader(path))
        } else {
            Self::parse(wheel_file, WhlMetadataReader::new(path)?)
        }
    }

    pub fn parse(
        wheel_file: WheelFile<'a>,
        mut metadata_reader: impl MetadataReader,
    ) -> anyhow::Result<Self> {
        let dist_info_dir = format!(
            "{project_name}-{version}.dist-info",
            project_name = wheel_file.raw_project_name,
            version = wheel_file.raw_version
        );
        let components = [&dist_info_dir, "METADATA"];
        let metadata_reader = metadata_reader.reader(&components)?;
        let metadata = Metadata::parse(io::read_to_string(metadata_reader)?.as_bytes())?;
        let mut requires_dists: Vec<Requirement<Url>> =
            Vec::with_capacity(metadata.requires_dist.len());
        for requires_dist in metadata.requires_dist {
            requires_dists.push(Requirement::from_str(&requires_dist)?)
        }
        let requires_python = if let Some(requires_python) = metadata.requires_python {
            Some(VersionSpecifiers::from_str(&requires_python)?)
        } else {
            None
        };

        Ok(Self {
            wheel_file,
            requires_dists,
            requires_python,
        })
    }
}

#[cfg(test)]
mod tests {

    use std::fs::File;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::str::FromStr;

    use pep440_rs::{Version, VersionSpecifiers};
    use pep508_rs::{PackageName, Requirement};
    use rstest::*;
    use testing::{tmp_dir, venv_python_exe};
    use zip::ZipArchive;

    use crate::wheel::{Tag, WheelFile, WheelMetadata};

    #[test]
    fn test_parse_wheel_file_name_simple() {
        let wheel_file = WheelFile::parse_file_name("cowsay-6.1-py3-none-any.whl").unwrap();
        assert_eq!("cowsay", wheel_file.raw_project_name);
        assert_eq!(
            PackageName::from_str("cowsay").unwrap(),
            wheel_file.project_name
        );
        assert_eq!("6.1", wheel_file.raw_version);
        assert_eq!(Version::from_str("6.1").unwrap(), wheel_file.version);
        assert_eq!(
            vec![Tag {
                python: "py3",
                abi: "none",
                platform: "any"
            }],
            wheel_file.tags
        )
    }

    #[test]
    fn test_parse_wheel_file_name_multiple_tags() {
        let wheel_file = WheelFile::parse_file_name(
            "psutil-7.2.2-cp314-cp314t-manylinux2014_aarch64.manylinux_2_17_aarch64.manylinux_2_17_aarch64.whl"
        ).unwrap();
        assert_eq!("psutil", wheel_file.raw_project_name);
        assert_eq!(
            PackageName::from_str("psutil").unwrap(),
            wheel_file.project_name
        );
        assert_eq!("7.2.2", wheel_file.raw_version);
        assert_eq!(Version::from_str("7.2.2").unwrap(), wheel_file.version);
        assert_eq!(
            vec![
                Tag {
                    python: "cp314",
                    abi: "cp314t",
                    platform: "manylinux2014_aarch64"
                },
                Tag {
                    python: "cp314",
                    abi: "cp314t",
                    platform: "manylinux_2_17_aarch64"
                },
                Tag {
                    python: "cp314",
                    abi: "cp314t",
                    platform: "manylinux_2_17_aarch64"
                }
            ],
            wheel_file.tags
        )
    }

    #[test]
    fn test_parse_wheel_file_name_build_tag() {
        let wheel_file =
            WheelFile::parse_file_name("cffi-1.14.3-2-cp39-cp39-macosx_10_9_x86_64.whl").unwrap();
        assert_eq!("cffi", wheel_file.raw_project_name);
        assert_eq!(
            PackageName::from_str("cffi").unwrap(),
            wheel_file.project_name
        );
        assert_eq!("1.14.3-2", wheel_file.raw_version);
        assert_eq!(Version::from_str("1.14.3-2").unwrap(), wheel_file.version);
        assert_eq!(
            vec![Tag {
                python: "cp39",
                abi: "cp39",
                platform: "macosx_10_9_x86_64"
            }],
            wheel_file.tags
        )
    }

    #[fixture]
    #[once]
    fn requests_2_32_5_whl(tmp_dir: PathBuf, venv_python_exe: PathBuf) -> PathBuf {
        let wheel_dir = tmp_dir.join("wheels");
        Command::new(venv_python_exe)
            .args([
                "-m",
                "pip",
                "download",
                "--no-deps",
                "--only-binary",
                ":all:",
                "--dest",
            ])
            .arg(&wheel_dir)
            .arg("requests==2.32.5")
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        let mut matches = glob::glob(wheel_dir.join("*.whl").to_str().unwrap()).unwrap();
        let wheel = matches.next().unwrap();
        assert!(matches.next().is_none());
        wheel.unwrap()
    }

    fn assert_requests_2_32_5_whl(wheel: &Path) {
        let wheel = WheelMetadata::try_from_path(wheel).unwrap();
        assert_eq!("requests", wheel.wheel_file.raw_project_name);
        assert_eq!("2.32.5", wheel.wheel_file.raw_version);
        assert_eq!(
            Some(VersionSpecifiers::from_str(">=3.9").unwrap()),
            wheel.requires_python
        );
        assert_eq!(
            vec![
                Requirement::from_str("charset_normalizer<4,>=2").unwrap(),
                Requirement::from_str("idna<4,>=2.5").unwrap(),
                Requirement::from_str("urllib3<3,>=1.21.1").unwrap(),
                Requirement::from_str("certifi>=2017.4.17").unwrap(),
                Requirement::from_str("PySocks!=1.5.7,>=1.5.6; extra == \"socks\"").unwrap(),
                Requirement::from_str("chardet<6,>=3.0.2; extra == \"use-chardet-on-py3\"")
                    .unwrap(),
            ],
            wheel.requires_dists
        );
    }

    #[rstest]
    fn test_parse_wheel_zip(requests_2_32_5_whl: &Path) {
        assert_requests_2_32_5_whl(requests_2_32_5_whl);
    }

    #[rstest]
    fn test_parse_wheel_chroot(requests_2_32_5_whl: &Path) {
        let tmp_dir = tempfile::tempdir().unwrap();
        let extract_dir = tmp_dir
            .path()
            .join(requests_2_32_5_whl.file_name().unwrap());
        let mut zip = ZipArchive::new(File::open(requests_2_32_5_whl).unwrap()).unwrap();
        zip.extract(&extract_dir).unwrap();
        assert_requests_2_32_5_whl(&extract_dir);
    }
}
