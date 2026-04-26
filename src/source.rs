// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::{env, io};

use anyhow::anyhow;
use fs_err as fs;
use url::Url;

pub fn to_path(source: String, fetch_dest_dir: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Ok(exists) = fs::exists(&source)
        && exists
    {
        Ok(source.into())
    } else {
        match Url::parse(&source) {
            Ok(url) if url.scheme() == "file" => Ok(url.path().to_string().into()),
            Ok(url) if url.scheme().is_empty() => Ok(source.into()),
            Ok(url) => {
                let dest_dir = match fetch_dest_dir {
                    Some(dest_dir) => Cow::Borrowed(dest_dir),
                    None => Cow::Owned(env::current_dir()?),
                };
                let file_name = url
                    .path()
                    .split("/")
                    .last()
                    .ok_or_else(|| anyhow!("The given url does not have a file name: {url}"))?;
                let dest_file = dest_dir.join(file_name);

                let mut contents = tempfile::NamedTempFile::new_in(&dest_dir)?;
                let mut response = reqwest::get(url)?;
                io::copy(&mut response, &mut contents)?;
                contents.persist(&dest_file)?;
                Ok(dest_file)
            }
            Err(_) => Ok(source.into()),
        }
    }
}
