// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::borrow::Cow;
use std::io;
use std::io::{Seek, Write};
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use fs_err as fs;
use fs_err::File;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use zip::write::{FileOptionExtension, FileOptions, SimpleFileOptions};
use zip::{ZipArchive, ZipWriter};

#[derive(Copy, Clone, EnumIter)]
pub enum Script {
    IdentifyInterpreter,
    VendoredVirtualenv,
    VenvPex,
    VenvPexRepl,
}

impl Script {
    pub const fn file_name(&self) -> &'static str {
        match self {
            Script::IdentifyInterpreter => "interpreter.py",
            Script::VendoredVirtualenv => "virtualenv.py",
            Script::VenvPex => "venv-pex.py",
            Script::VenvPexRepl => "venv-pex-repl.py",
        }
    }
}

pub enum Scripts {
    #[cfg(feature = "embedded")]
    Embedded,
    Loose(PathBuf),
    Zipped(ZipArchive<File>),
}

const ZIP_REL_PATH: &str = "__pex__/.scripts";
static HOST_REL_PATH: LazyLock<PathBuf> = LazyLock::new(|| ZIP_REL_PATH.split("/").collect());

impl Scripts {
    pub fn read(&mut self, script: Script) -> anyhow::Result<Cow<'static, str>> {
        match self {
            #[cfg(feature = "embedded")]
            Scripts::Embedded => Ok(Cow::Borrowed(match script {
                Script::IdentifyInterpreter => include_str!("interpreter.py"),
                Script::VendoredVirtualenv => include_str!(env!("VIRTUALENV_PY")),
                Script::VenvPex => include_str!("venv-pex.py"),
                Script::VenvPexRepl => include_str!("venv-pex-repl.py"),
            })),
            Scripts::Loose(base_dir) => {
                let resource_path = base_dir
                    .join(HOST_REL_PATH.as_path())
                    .join(script.file_name());
                Ok(Cow::Owned(fs::read_to_string(resource_path)?))
            }
            Scripts::Zipped(zip) => {
                let resource_path =
                    format!("{ZIP_REL_PATH}/{file_name}", file_name = script.file_name());
                let entry = zip.by_name(&resource_path)?;
                Ok(Cow::Owned(io::read_to_string(entry)?))
            }
        }
    }

    pub fn inject<'a, T: FileOptionExtension + Copy>(
        &mut self,
        zip: &'a mut ZipWriter<impl Write + Seek>,
        file_options: FileOptions<'a, T>,
    ) -> anyhow::Result<()> {
        let directory_options = SimpleFileOptions::default();
        zip.add_directory(ZIP_REL_PATH, directory_options)?;
        for resource_path in Script::iter() {
            let text = self.read(resource_path)?;
            zip.start_file(
                format!(
                    "{ZIP_REL_PATH}/{script}",
                    script = resource_path.file_name()
                ),
                file_options,
            )?;
            zip.write_all(text.as_bytes())?;
        }
        Ok(())
    }

    pub fn write(&mut self, dest_dir: &Path) -> anyhow::Result<()> {
        let scripts_dir = dest_dir.join(HOST_REL_PATH.as_path());
        fs::create_dir_all(&scripts_dir)?;
        for resource_path in Script::iter() {
            let text = self.read(resource_path)?;
            let mut file = File::create_new(scripts_dir.join(resource_path.file_name()))?;
            file.write_all(text.as_bytes())?;
        }
        Ok(())
    }
}

macro_rules! generate_script_type {
    ( $script_type:ident ) => {
        pub struct $script_type<'a>(Cow<'a, str>);

        impl<'a> $script_type<'a> {
            pub fn read(scripts: &mut Scripts) -> anyhow::Result<$script_type<'a>> {
                let text = scripts.read(Script::$script_type)?;
                Ok($script_type(text))
            }

            pub fn contents(&self) -> &str {
                self.0.as_ref()
            }
        }
    };
}

generate_script_type!(IdentifyInterpreter);
generate_script_type!(VendoredVirtualenv);
generate_script_type!(VenvPex);
generate_script_type!(VenvPexRepl);
