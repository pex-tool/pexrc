// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs};

use anyhow::bail;
use bstr::ByteSlice;
use const_format::concatcp;
use strum::{EnumCount, IntoEnumIterator};
use strum_macros::{EnumCount, EnumIter};
use which::which_in_global;

use crate::config::{Build, CargoBinstall, Clib, DownloadArchive, Glibc};
use crate::downloads::ensure_download;

pub(crate) struct ToolBox<'a> {
    clib: Clib<'a>,
    binstall: CargoBinstall<'a>,
    zig_version: &'a str,
    glibc: Glibc<'a>,
    binstall_tools: Vec<BinstallTool>,
    downloads: Vec<(&'static str, DownloadArchive<'a>)>,
}

impl<'a> From<Build<'a>> for ToolBox<'a> {
    fn from(build: Build<'a>) -> Self {
        Self {
            clib: build.clib,
            binstall: build.cargo_binstall,
            zig_version: build.zig_version,
            glibc: build.glibc,
            binstall_tools: BinstallTool::iter().collect::<Vec<_>>(),
            downloads: vec![("SDKROOT", build.mac_osx_sdk)],
        }
    }
}

impl<'a> ToolBox<'a> {
    pub(crate) fn find_tools(self, install_dirs: InstallDirs) -> anyhow::Result<ToolInventory<'a>> {
        let mut missing: Vec<BinstallTool> = Vec::with_capacity(BinstallTool::COUNT);
        let search_path = install_dirs.search_path()?;
        let zig = if let Some(zig) = find_zig(
            &["zig", "python-zig"],
            self.zig_version,
            search_path.as_ref(),
        ) {
            Zig::Found(zig)
        } else {
            Zig::MissingVersion(self.zig_version)
        };
        for tool in self.binstall_tools {
            if let Ok(Some(exe)) = which_in_global(tool.binary_name(), Some(&search_path))
                .map(|mut found| found.next())
            {
                eprintln!(
                    "Found {tool} at {exe}",
                    tool = tool.binary_name(),
                    exe = exe.display()
                );
            } else {
                missing.push(tool)
            }
        }
        Ok(ToolInventory {
            clib: self.clib,
            binstall: self.binstall,
            downloads: self.downloads,
            zig,
            glibc: self.glibc,
            missing,
            install_dirs,
        })
    }
}

#[derive(Clone)]
pub struct FoundTool {
    pub env_var: &'static str,
    pub path: PathBuf,
}

pub fn find_zig(binary_names: &[&str], version: &str, search_path: &OsStr) -> Option<FoundTool> {
    for binary_name in binary_names {
        if let Ok(zig_paths) = which_in_global(binary_name, Some(search_path)) {
            for zig in zig_paths {
                if let Some(zig_version) = get_zig_version(&zig)
                    && zig_version == version
                {
                    return Some(FoundTool {
                        env_var: "CARGO_ZIGBUILD_ZIG_PATH",
                        path: zig,
                    });
                }
            }
        }
    }
    None
}

fn get_zig_version(zig: &Path) -> Option<String> {
    Command::new(zig)
        .arg("version")
        .stdout(Stdio::piped())
        .spawn()
        .ok()
        .and_then(|child| child.wait_with_output().ok())
        .and_then(|result| {
            if result.status.success() {
                result.stdout.to_str().ok().map(str::trim).map(String::from)
            } else {
                None
            }
        })
}

pub struct InstallDirs {
    bin_dir: PathBuf,
    download_dir: PathBuf,
}

impl InstallDirs {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            bin_dir: cache_dir.join("bin"),
            download_dir: cache_dir.join("downloads"),
        }
    }

    fn search_path(&self) -> anyhow::Result<Cow<'_, OsStr>> {
        if let Some(search_path) = env::var_os("PATH").as_deref().map(env::split_paths) {
            let search_path = env::join_paths(search_path.chain([self.bin_dir.clone()]))?;
            Ok(Cow::Owned(search_path))
        } else {
            Ok(Cow::Borrowed(self.bin_dir.as_os_str()))
        }
    }
}

#[derive(EnumCount, EnumIter)]
pub enum BinstallTool {
    CargoXwin,
    CargoZigbuild,
    Uv,
}

impl BinstallTool {
    pub fn binary_name(&self) -> &'static str {
        match *self {
            BinstallTool::CargoXwin => "cargo-xwin",
            BinstallTool::CargoZigbuild => "cargo-zigbuild",
            BinstallTool::Uv => "uv",
        }
    }
}

pub enum Zig<'a> {
    Found(FoundTool),
    MissingVersion(&'a str),
}

impl<'a> Zig<'a> {
    pub fn found(&self) -> bool {
        matches!(*self, Zig::Found(_))
    }

    pub fn missing_version(&'a self) -> Option<&'a str> {
        match *self {
            Zig::MissingVersion(version) => Some(version),
            _ => None,
        }
    }
}

pub struct ToolInventory<'a> {
    clib: Clib<'a>,
    glibc: Glibc<'a>,
    binstall: CargoBinstall<'a>,
    zig: Zig<'a>,
    downloads: Vec<(&'static str, DownloadArchive<'a>)>,
    missing: Vec<BinstallTool>,
    install_dirs: InstallDirs,
}

pub enum ToolInstallation<'a> {
    Success((Clib<'a>, Glibc<'a>, Vec<FoundTool>)),
    Failure((Zig<'a>, Vec<BinstallTool>, OsString)),
}

impl<'a> ToolInventory<'a> {
    pub fn ensure_tools_installed(
        self,
        cargo: &Path,
        install_missing_tools: bool,
    ) -> anyhow::Result<ToolInstallation<'a>> {
        let tool_search_path =
            if let Some(search_path) = env::var_os("PATH").as_deref().map(env::split_paths) {
                let search_path =
                    env::join_paths(search_path.chain([self.install_dirs.bin_dir.clone()]))?;
                Cow::Owned(search_path)
            } else {
                Cow::Borrowed(self.install_dirs.bin_dir.as_os_str())
            };

        let mut found_tools = Vec::new();

        if !self.missing.is_empty() || !self.zig.found() {
            if install_missing_tools {
                let zig = install_tools(
                    cargo,
                    &self.binstall,
                    self.missing.as_slice(),
                    &self.zig,
                    &self.install_dirs,
                    &tool_search_path,
                )?;
                found_tools.push(zig.into_owned());
            } else {
                return Ok(ToolInstallation::Failure((
                    self.zig,
                    self.missing,
                    tool_search_path.into_owned(),
                )));
            }
        }
        for (env_var, download) in &self.downloads {
            let download_path = ensure_download(download, &self.install_dirs.download_dir)?;
            found_tools.push(FoundTool {
                env_var,
                path: download_path,
            });
        }
        Ok(ToolInstallation::Success((
            self.clib,
            self.glibc,
            found_tools,
        )))
    }
}

fn install_tools<'a>(
    cargo: &Path,
    cargo_binstall: &CargoBinstall,
    tools: &[BinstallTool],
    zig: &'a Zig,
    install_dirs: &InstallDirs,
    search_path: &OsStr,
) -> anyhow::Result<Cow<'a, FoundTool>> {
    for tool in tools {
        binstall(
            cargo_binstall,
            install_dirs,
            search_path,
            cargo,
            tool.binary_name(),
        )?;
    }

    match zig {
        Zig::Found(zig) => Ok(Cow::Borrowed(zig)),
        Zig::MissingVersion(version) => {
            let zig_requirement = format!("ziglang=={version}");
            fs::create_dir_all(&install_dirs.bin_dir)?;
            let result = Command::new("uv")
                .args(["tool", "install", "--force", &zig_requirement])
                .env("UV_TOOL_BIN_DIR", install_dirs.bin_dir.as_os_str())
                .stderr(Stdio::piped())
                .spawn()?
                .wait_with_output()?;
            if !result.status.success() {
                bail!(
                    "Failed to install zig {version} via `uv tool install {zig_requirement}`:\n\
                {stderr}",
                    stderr = result.stderr.to_str_lossy()
                )
            } else if let Some(zig) = find_zig(&["python-zig"], version, search_path) {
                Ok(Cow::Owned(zig))
            } else {
                bail!(
                    "Failed to find zig on PATH={search_path} after installing via \
                    `uv tool install --force {zig_requirement}`.",
                    search_path = search_path.to_string_lossy()
                )
            }
        }
    }
}

const CARGO_BINSTALL_FILE_NAME: &str = concatcp!("cargo-binstall", env::consts::EXE_SUFFIX);

fn binstall(
    cargo_binstall: &CargoBinstall,
    install_dirs: &InstallDirs,
    search_path: &OsStr,
    cargo: &Path,
    spec: &str,
) -> anyhow::Result<()> {
    if let Ok(Some(exe)) =
        which_in_global("cargo-binstall", Some(search_path)).map(|mut matches| matches.next())
    {
        eprintln!("Found cargo-binstall at {exe}", exe = exe.display());
    } else {
        let target = env::var("TARGET")?;
        if let Some(download) = cargo_binstall.download_for(&target)? {
            let cargo_binstall = ensure_download(&download, &install_dirs.download_dir)?
                .join(CARGO_BINSTALL_FILE_NAME);
            let cargo_binstall_fp = File::open(&cargo_binstall)?;
            cargo_binstall_fp.lock()?;
            let dst = install_dirs.bin_dir.join(CARGO_BINSTALL_FILE_NAME);
            if dst.exists() {
                fs::remove_file(&dst)?;
            } else {
                fs::create_dir_all(&install_dirs.bin_dir)?;
            }
            fs::hard_link(&cargo_binstall, &dst)?;
        } else {
            let spec = format!("cargo-binstall@{version}", version = cargo_binstall.version);
            let result = Command::new(cargo)
                .args(["install", "--locked", &spec])
                .stderr(Stdio::piped())
                .spawn()?
                .wait_with_output()?;
            if !result.status.success() {
                bail!(
                    "Failed to install cargo-binstall to bootstrap tools with:\n{stderr}",
                    stderr = result.stderr.to_str_lossy()
                )
            }
        }
    }

    let result = Command::new(cargo)
        .env("PATH", search_path)
        .args(["binstall", "--no-confirm", spec])
        .stderr(Stdio::piped())
        .spawn()?
        .wait_with_output()?;
    if !result.status.success() {
        bail!(
            "Failed to install {spec}:\n{stderr}",
            stderr = result.stderr.to_str_lossy()
        )
    }
    Ok(())
}
