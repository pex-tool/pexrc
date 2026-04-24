// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter, Write};
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, bail};
use indexmap::IndexSet;
use interpreter::Tag;
use pep440_rs::Version;
use pep508_rs::PackageName;

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

    pub fn data_dir(&self) -> PathBuf {
        self.pnav_dir("data")
    }

    pub fn dist_info_dir(&self) -> PathBuf {
        self.pnav_dir("dist-info")
    }

    pub fn pex_info_dir(&self) -> PathBuf {
        self.pnav_dir("pex-info")
    }

    fn pnav_dir(&self, name: &str) -> PathBuf {
        format!(
            "{project_name}-{version}.{name}",
            project_name = self.raw_project_name,
            version = self.raw_version
        )
        .into()
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use interpreter::Tag;
    use pep440_rs::Version;
    use pep508_rs::PackageName;

    use crate::wheel::file::WheelFile;

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
}
