// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter, Write};
use std::str::FromStr;

use anyhow::{anyhow, bail};
use indexmap::IndexSet;
use interpreter::Tag;
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement};
use python_pkginfo::Metadata;
use url::Url;

pub struct WheelFile<'a> {
    pub(crate) file_name: &'a str,
    pub(crate) raw_project_name: &'a str,
    pub(crate) project_name: PackageName,
    pub(crate) raw_version: &'a str,
    pub(crate) version: Version,
    pub(crate) build_tag: Option<&'a str>,
    pub(crate) tags: Vec<Tag<'a>>,
}

impl<'a> WheelFile<'a> {
    pub(crate) fn parse_file_name(file_name: &'a str) -> anyhow::Result<Self> {
        // See: https://packaging.python.org/en/latest/specifications/binary-distribution-format/#file-name-convention
        // {distribution}-{version}(-{build tag})?-{python tag}-{abi tag}-{platform tag}.whl
        if !file_name.ends_with(".whl") {
            bail!("Not a wheel file name {file_name}")
        }

        let (raw_project_name, raw_version, build_tag, python_tag, abi_tag, platform_tag) = {
            let mut trailing_components = file_name[0..file_name.len() - 4].rsplitn(4, "-");
            let platform_tag = trailing_components
                .next()
                .ok_or_else(|| anyhow!("Failed to parse platform tag from {file_name}"))?;
            let abi_tag = trailing_components
                .next()
                .ok_or_else(|| anyhow!("Failed to parse abi tag from {file_name}"))?;
            let python_tag = trailing_components
                .next()
                .ok_or_else(|| anyhow!("Failed to parse python tag from {file_name}"))?;
            let rest = trailing_components
                .next()
                .ok_or_else(|| anyhow!("Failed to parse wheel tags from {file_name}"))?;

            let mut leading_components = rest.splitn(3, "-");
            let project_name = leading_components
                .next()
                .ok_or_else(|| anyhow!("Failed to parse project name from {file_name}"))?;
            let version = leading_components
                .next()
                .ok_or_else(|| anyhow!("Failed to parse version from {file_name}"))?;
            let build_tag = leading_components.next();

            (
                project_name,
                version,
                build_tag,
                python_tag,
                abi_tag,
                platform_tag,
            )
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
            file_name,
            raw_project_name,
            project_name,
            raw_version,
            version,
            build_tag,
            tags,
        })
    }

    fn write_tag_component(
        &self,
        f: &mut Formatter<'_>,
        extract_tag_component: impl Fn(&Tag<'a>) -> &'a str,
    ) -> std::fmt::Result {
        f.write_char('-')?;
        for (idx, python) in self
            .tags
            .iter()
            .map(extract_tag_component)
            .collect::<IndexSet<_>>()
            .into_iter()
            .enumerate()
        {
            if idx > 0 {
                f.write_char('.')?;
            }
            f.write_str(python)?;
        }
        Ok(())
    }
}

impl<'a> Display for WheelFile<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{project_name}-{version}",
            project_name = self.raw_project_name,
            version = self.raw_version
        )?;
        if let Some(build_tag) = self.build_tag {
            write!(f, "-{build_tag}")?;
        }
        self.write_tag_component(f, |tag| tag.python)?;
        self.write_tag_component(f, |tag| tag.abi)?;
        self.write_tag_component(f, |tag| tag.platform)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct WheelMetadata<'a> {
    pub file_name: &'a str,
    pub raw_project_name: &'a str,
    pub project_name: PackageName,
    pub raw_version: &'a str,
    pub version: Version,
    pub requires_dists: Vec<Requirement<Url>>,
    pub requires_python: Option<VersionSpecifiers>,
}

pub trait MetadataReader<'a> {
    fn read(
        &mut self,
        wheel_file_name: &'a str,
        path_components: &[&str],
    ) -> anyhow::Result<String>;
}

impl<'a> WheelMetadata<'a> {
    pub fn parse(
        wheel_file: WheelFile<'a>,
        metadata_reader: &mut impl MetadataReader<'a>,
    ) -> anyhow::Result<Self> {
        let dist_info_dir = format!(
            "{project_name}-{version}.dist-info",
            project_name = wheel_file.raw_project_name,
            version = wheel_file.raw_version
        );
        let components = [&dist_info_dir, "METADATA"];
        let metadata = Metadata::parse(
            metadata_reader
                .read(wheel_file.file_name, &components)?
                .as_bytes(),
        )?;
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
            file_name: wheel_file.file_name,
            raw_project_name: wheel_file.raw_project_name,
            project_name: wheel_file.project_name,
            raw_version: wheel_file.raw_version,
            version: wheel_file.version,
            requires_dists,
            requires_python,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::str::FromStr;

    use fs_err::File;
    use interpreter::Tag;
    use pep440_rs::{Version, VersionSpecifiers};
    use pep508_rs::{PackageName, Requirement};
    use rstest::*;
    use testing::{tmp_dir, venv_python_exe};
    use zip::ZipArchive;

    use crate::wheel::{MetadataReader, WheelFile, WheelMetadata};

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
        assert_eq!("1.14.3", wheel_file.raw_version);
        assert_eq!(Some("2"), wheel_file.build_tag);
        assert_eq!(Version::from_str("1.14.3").unwrap(), wheel_file.version);
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
        assert!(
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
                .unwrap()
                .success()
        );
        let mut matches = glob::glob(wheel_dir.join("*.whl").to_str().unwrap()).unwrap();
        let wheel = matches.next().unwrap();
        assert!(matches.next().is_none());
        wheel.unwrap()
    }

    #[rstest]
    fn test_parse_wheel_chroot(requests_2_32_5_whl: &Path) {
        let file_name = requests_2_32_5_whl
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap();
        let wheel_file = WheelFile::parse_file_name(file_name).unwrap();
        assert_eq!("requests", wheel_file.raw_project_name);
        assert_eq!("2.32.5", wheel_file.raw_version);

        struct RequestsMetadataReader(ZipArchive<File>);
        impl<'a> MetadataReader<'a> for RequestsMetadataReader {
            fn read(&mut self, _: &str, path_components: &[&str]) -> anyhow::Result<String> {
                Ok(io::read_to_string(
                    self.0.by_name(&path_components.join("/"))?,
                )?)
            }
        }
        let mut metadata_reader = RequestsMetadataReader(
            ZipArchive::new(File::open(requests_2_32_5_whl).unwrap()).unwrap(),
        );

        let wheel = WheelMetadata::parse(wheel_file, &mut metadata_reader).unwrap();
        assert_eq!(
            PackageName::new("requests".into()).unwrap(),
            wheel.project_name
        );
        assert_eq!(Version::new([2, 32, 5]), wheel.version);
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
}
