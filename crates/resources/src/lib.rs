// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::io::{Seek, Write};

use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use zip::ZipWriter;
use zip::write::{FileOptionExtension, FileOptions, SimpleFileOptions};

#[cfg(feature = "embedded")]
pub mod embedded;

#[derive(Copy, Clone, EnumIter)]
pub enum ResourcePath {
    InterpreterIdentificationScript,
    VendoredVirtualenvScript,
    VenvPexScript,
    VenvPexReplScript,
}

impl ResourcePath {
    pub fn script_name(&self) -> &'static str {
        match self {
            ResourcePath::InterpreterIdentificationScript => "interpreter.py",
            ResourcePath::VendoredVirtualenvScript => "virtualenv.py",
            ResourcePath::VenvPexScript => "venv-pex.py",
            ResourcePath::VenvPexReplScript => "venv-pex-repl.py",
        }
    }
}

pub trait Resources<'a> {
    fn read(&mut self, path: ResourcePath) -> anyhow::Result<Cow<'a, str>>;

    fn inject_scripts<T: FileOptionExtension + Copy>(
        &mut self,
        zip: &mut ZipWriter<impl Write + Seek>,
        file_options: FileOptions<'a, T>,
    ) -> anyhow::Result<()> {
        let directory_options = SimpleFileOptions::default();
        zip.add_directory("__pex__/.scripts", directory_options)?;
        for resource_path in ResourcePath::iter() {
            let text = self.read(resource_path)?;
            zip.start_file(
                format!(
                    "__pex__/.scripts/{script}",
                    script = resource_path.script_name()
                ),
                file_options,
            )?;
            zip.write_all(text.as_bytes())?;
        }
        Ok(())
    }
}

macro_rules! generate_script_type {
    ( $resource_path:ident ) => {
        pub struct $resource_path<'a>(Cow<'a, str>);

        impl<'a> $resource_path<'a> {
            pub fn read(resources: &mut impl Resources<'a>) -> anyhow::Result<$resource_path<'a>> {
                let text = resources.read(ResourcePath::$resource_path)?;
                Ok($resource_path(text))
            }

            pub fn contents(&self) -> &str {
                self.0.as_ref()
            }
        }
    };
}

generate_script_type!(InterpreterIdentificationScript);
generate_script_type!(VendoredVirtualenvScript);
generate_script_type!(VenvPexScript);
generate_script_type!(VenvPexReplScript);
