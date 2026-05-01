// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::str::FromStr;

use anyhow::anyhow;
use mailparse::MailHeaderMap;
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement};
use python_pkginfo::Metadata;
use url::Url;

use crate::wheel::file::WheelFile;

pub struct WheelMetadata<'a> {
    pub file_name: &'a str,
    pub raw_project_name: &'a str,
    pub project_name: PackageName,
    pub raw_version: &'a str,
    pub version: Version,
    pub requires_dists: Vec<Requirement<Url>>,
    pub requires_python: Option<VersionSpecifiers>,
    pub root_is_purelib: bool,
}

pub(crate) trait MetadataReader<'a> {
    fn read(
        &mut self,
        wheel_file_name: &'a str,
        path_components: &[&str],
    ) -> anyhow::Result<String>;
}

fn parse_root_is_purelib_from_wheel(content: &[u8]) -> anyhow::Result<bool> {
    let msg = mailparse::parse_mail(content)?;
    let headers = msg.get_headers();
    let header = headers
        .get_first_header("Root-Is-Purelib")
        .ok_or_else(|| anyhow!(""))?;
    Ok(matches!(
        rfc2047_decoder::decode(header.get_value_raw())?.as_str(),
        "true" | "True"
    ))
}

impl<'a> WheelMetadata<'a> {
    pub(crate) fn parse(
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

        let components = [&dist_info_dir, "WHEEL"];
        let root_is_purelib = parse_root_is_purelib_from_wheel(
            metadata_reader
                .read(wheel_file.file_name, &components)?
                .as_bytes(),
        )?;

        Ok(Self {
            file_name: wheel_file.file_name,
            raw_project_name: wheel_file.raw_project_name,
            project_name: wheel_file.project_name,
            raw_version: wheel_file.raw_version,
            version: wheel_file.version,
            requires_dists,
            requires_python,
            root_is_purelib,
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
    use pep440_rs::{Version, VersionSpecifiers};
    use pep508_rs::{PackageName, Requirement};
    use rstest::*;
    use testing::{tmp_dir, venv_python_exe};
    use zip::ZipArchive;

    use crate::wheel::file::WheelFile;
    use crate::wheel::metadata::{MetadataReader, WheelMetadata};

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
