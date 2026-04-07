// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::fs::File;
use std::io;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::LazyLock;

use anyhow::anyhow;
use pex::{Layout, Pex};
use target::Target;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const SHEBANG_PREFIX: &str = "\n#!";
const SHEBANG_SUFFIX: &str = "\n";

pub enum ProxySource<'a> {
    Bytes(&'a [u8]),
    Pex(&'a Pex<'a>),
}

pub fn create(
    proxy_source: ProxySource,
    interpreter: &Path,
    mut target_python: File,
    script: Option<String>,
) -> anyhow::Result<()> {
    match proxy_source {
        ProxySource::Bytes(mut bytes) => {
            io::copy(&mut bytes, &mut target_python)?;
        }
        ProxySource::Pex(pex) => match pex.layout {
            Layout::Loose | Layout::Packed => {
                let mut python_proxy = read_python_proxy_from_dir(pex.path)?;
                io::copy(&mut python_proxy, &mut target_python)?;
            }
            Layout::ZipApp => {
                let mut pex_zip = ZipArchive::new(File::open(pex.path)?)?;
                let mut python_proxy = read_python_proxy_from_zip(&mut pex_zip)?;
                io::copy(&mut python_proxy, &mut target_python)?;
            }
        },
    }

    let shebang_python = interpreter.as_os_str();
    if let Some(script) = script {
        let mut script_zip = ZipWriter::new(&target_python);
        script_zip.start_file(
            "__main__.py",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        script_zip.write_all(script.as_bytes())?;
        script_zip.set_comment(format!(
            "{SHEBANG_PREFIX}{shebang_python}{SHEBANG_SUFFIX}",
            shebang_python = shebang_python.to_str().ok_or_else(|| anyhow!(
                "The shebang python path is not UTF-8: {shebang_python}",
                shebang_python = shebang_python.display()
            ))?
        ));
        script_zip.finish()?;
    } else {
        target_python.write_all(SHEBANG_PREFIX.as_bytes())?;
        target_python.write_all(shebang_python.as_encoded_bytes())?;
        target_python.write_all(SHEBANG_SUFFIX.as_bytes())?;
    }

    platform::mark_executable(&mut target_python)?;
    Ok(())
}

static PYTHON_PROXY_FILE_NAME: LazyLock<anyhow::Result<String>> = LazyLock::new(|| {
    let current_target = Target::current()?;
    Ok(current_target.fully_qualified_binary_name("python-proxy", None))
});

fn python_proxy_file_name<'a>() -> anyhow::Result<&'a str> {
    PYTHON_PROXY_FILE_NAME
        .as_deref()
        .map_err(|err| anyhow!("{err}"))
}

fn read_python_proxy_from_dir(pex_dir: &Path) -> anyhow::Result<impl Read> {
    Ok(File::open(
        pex_dir
            .join("__pex__")
            .join(".proxies")
            .join(python_proxy_file_name()?),
    )?)
}

fn read_python_proxy_from_zip(
    pex_zip: &mut ZipArchive<impl Read + Seek>,
) -> anyhow::Result<impl Read> {
    Ok(pex_zip.by_name(&format!(
        "__pex__/.proxies/{python_proxy_name}",
        python_proxy_name = python_proxy_file_name()?
    ))?)
}
