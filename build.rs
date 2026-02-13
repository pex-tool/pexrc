// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, io};

use anyhow::bail;
use bstr::ByteSlice;
use const_format::concatcp;
use itertools::Itertools;
use pexrc_build_system::{
    BinstallTool,
    CargoBinstall,
    DownloadArchive,
    FoundTool,
    ToolInventory,
    Zig,
    classify_targets,
    find_zig,
    inventory_tools,
};
use sha2::Digest;
use which::which_in_global;

struct InstallDirs {
    bin_dir: PathBuf,
    download_dir: PathBuf,
}

impl InstallDirs {
    fn new(cache_dir: PathBuf) -> Self {
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

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cargo: PathBuf = env::var("CARGO")?.into();

    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("target")
    };
    let install_dirs = if let Some(cache_dir) = dirs::cache_dir() {
        InstallDirs::new(cache_dir.join("pexrc-dev"))
    } else {
        let cache_dir = target_dir.join(".pexrc-dev");
        println!(
            "cargo::warning=Failed to discover the user cache dir; using {cache_dir}",
            cache_dir = cache_dir.display()
        );
        InstallDirs::new(cache_dir)
    };
    let tool_search_path = install_dirs.search_path()?;

    let data = {
        let manifest_path = env::var("CARGO_MANIFEST_PATH")?;
        fs::read_to_string(manifest_path)?
    };
    let tool_inventory = inventory_tools(data.as_str(), tool_search_path)?;
    let found_tools = ensure_tools_installed(&cargo, &tool_inventory, &install_dirs)?;

    let rust_toolchain_contents = fs::read_to_string("rust-toolchain")?;
    let targets = classify_targets(rust_toolchain_contents.as_str(), &tool_inventory.glibc)?;

    custom_cargo_build(
        &cargo,
        ["xwin", "build"],
        &tool_inventory,
        &found_tools,
        targets.iter_xwin_targets(),
    )?;
    custom_cargo_build(
        &cargo,
        ["zigbuild"],
        &tool_inventory,
        &found_tools,
        targets.iter_zigbuild_targets(),
    )?;

    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap().into();
    let clibs_dir = out_dir.join("clibs");
    fs::create_dir_all(&clibs_dir)?;
    println!(
        "cargo::rustc-env=CLIBS_DIR={clibs_dir}",
        clibs_dir = clibs_dir.display()
    );

    for target in targets.iter_targets() {
        let clib_name = target.shared_library_name("pexrc");
        let clib = target_dir
            .join(target.as_str())
            .join(tool_inventory.clib.profile)
            .join(&clib_name);
        if !clib.exists() {
            eprintln!(
                "The clib for {target} does not exist at {clib_path}!",
                clib_path = clib.display(),
                target = target.as_str(),
            );
        }
        io::copy(
            &mut File::open(clib)?,
            &mut zstd::Encoder::new(
                File::create(
                    clibs_dir.join(format!("{target}.{clib_name}", target = target.as_str())),
                )?,
                tool_inventory.clib.compression_level,
            )?,
        )?;
    }

    Ok(())
}

fn custom_cargo_build<'a>(
    cargo: &Path,
    custom_build_args: impl IntoIterator<Item = &'a str>,
    tool_inventory: &ToolInventory,
    found_tools: impl IntoIterator<Item = &'a FoundTool>,
    targets: impl Iterator<Item = &'a str>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new(cargo);
    let cmd = cmd
        .stderr(Stdio::piped())
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TERM_COLOR", "always");
    for found_tool in found_tools {
        println!(
            "cargo::rustc-env={env_var}={path}",
            env_var = found_tool.env_var,
            path = found_tool.path.display()
        );
        cmd.env(found_tool.env_var, &found_tool.path);
    }
    cmd.args(custom_build_args).args([
        "--package",
        "clib",
        "--profile",
        tool_inventory.clib.profile,
    ]);
    for target in targets {
        cmd.args(["--target", target]);
    }

    let result = cmd.spawn()?.wait_with_output()?;
    if result.status.success() {
        return Ok(());
    }
    bail!(
        "Failed to compile clib with exit code {exit_code}:\n{exe} \\\n  {args}\n{output}",
        exit_code = result.status,
        exe = cmd.get_program().to_string_lossy(),
        args = cmd.get_args().map(OsStr::to_string_lossy).join(" \\\n  "),
        output = result.stderr.to_str_lossy()
    )
}

fn ensure_tools_installed(
    cargo: &Path,
    tool_inventory: &ToolInventory,
    install_dirs: &InstallDirs,
) -> anyhow::Result<Vec<FoundTool>> {
    let tool_search_path =
        if let Some(search_path) = env::var_os("PATH").as_deref().map(env::split_paths) {
            let search_path = env::join_paths(search_path.chain([install_dirs.bin_dir.clone()]))?;
            Cow::Owned(search_path)
        } else {
            Cow::Borrowed(install_dirs.bin_dir.as_os_str())
        };

    let mut found_tools = Vec::new();
    if !tool_inventory.missing.is_empty() || !tool_inventory.zig.found() {
        if let Some(value) = env::var_os("PEXRC_INSTALL_TOOLS")
            && value == "1"
        {
            let mut installed_tools = install_tools(
                cargo,
                &tool_inventory.binstall,
                tool_inventory.missing.as_slice(),
                &tool_inventory.zig,
                install_dirs,
                &tool_search_path,
            )?;
            found_tools.append(&mut installed_tools);
        } else {
            bail!(
                "The following tools are required but are not installed: {tools}\n\
                Searched PATH: {search_path}\n\
                Re-run with PEXRC_INSTALL_TOOLS=1 to let the build script install these tools.",
                tools = tool_inventory
                    .missing
                    .iter()
                    .map(|tool| Cow::Borrowed(tool.binary_name()))
                    .chain(
                        tool_inventory
                            .zig
                            .missing_version()
                            .iter()
                            .map(|version| Cow::Owned(format!("zig@{version}")))
                    )
                    .join(" "),
                search_path = tool_search_path.display()
            );
        }
    }
    for found in ensure_downloads(&tool_inventory.downloads, install_dirs)? {
        found_tools.push(found);
    }
    Ok(found_tools)
}

fn ensure_downloads<'a>(
    downloads: impl AsRef<[(&'static str, DownloadArchive<'a>)]>,
    install_dirs: &'a InstallDirs,
) -> anyhow::Result<Vec<FoundTool>> {
    if downloads.as_ref().is_empty() {
        return Ok(Vec::new());
    }
    let mut download_paths: Vec<FoundTool> = Vec::with_capacity(downloads.as_ref().len());
    for (env_var, download) in downloads.as_ref() {
        let download_path = ensure_download(&install_dirs.download_dir, download)?;
        download_paths.push(FoundTool {
            env_var,
            path: download_path,
        });
    }
    Ok(download_paths)
}

enum ArchiveType {
    TarLzma,
    TarGzip,
    Zip,
}

impl TryFrom<&str> for ArchiveType {
    type Error = anyhow::Error;

    fn try_from(path: &str) -> anyhow::Result<Self> {
        let archive_type = if [".tar.gz", ".tgz"].iter().any(|ext| path.ends_with(ext)) {
            ArchiveType::TarGzip
        } else if [".tar.xz", ".tar.lzma", ".tlz"]
            .iter()
            .any(|ext| path.ends_with(ext))
        {
            ArchiveType::TarLzma
        } else if [".zip"].iter().any(|ext| path.ends_with(ext)) {
            ArchiveType::Zip
        } else {
            bail!("No support for downloading archives of this sort: {path}");
        };
        Ok(archive_type)
    }
}

fn ensure_download(download_dir: &Path, download: &DownloadArchive) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(download_dir)?;
    let dst_dir = download_dir.join(download.fingerprint.hash);
    let downloaded_path = Ok(if let Some(prefix) = download.prefix {
        dst_dir.join(prefix)
    } else {
        dst_dir.clone()
    });

    // Double-checked lock.
    if dst_dir.exists() {
        return downloaded_path;
    }
    let lock_file = File::create(dst_dir.with_added_extension("lck"))?;
    lock_file.lock()?;
    if dst_dir.exists() {
        return downloaded_path;
    }

    let hasher = match download.fingerprint.algorithm {
        "sha256" => sha2::Sha256::new(),
        algorithm => bail!("No support for {algorithm} hashes."),
    };

    let url = reqwest::Url::parse(download.url.as_ref())?;
    let archive_type = ArchiveType::try_from(url.path())?;

    let response = reqwest::blocking::get(url)?;
    if let Some(actual_size) = response.content_length()
        && actual_size != download.size
    {
        bail!(
            "Expected {url} to be {expected_size} bytes but is {actual_size} bytes.",
            url = download.url,
            expected_size = download.size
        );
    }
    let download_dir = tempfile::TempDir::new_in(download_dir)?;
    let mut digest_reader =
        DigestReader::new(download.size, hasher, response, download.url.as_ref());
    match archive_type {
        ArchiveType::TarGzip => {
            let mut tar_stream =
                tar::Archive::new(flate2::read::GzDecoder::new(&mut digest_reader));
            tar_stream.unpack(download_dir.path())?;
        }
        ArchiveType::TarLzma => {
            let mut tar_stream = tar::Archive::new(xz2::read::XzDecoder::new(&mut digest_reader));
            tar_stream.unpack(download_dir.path())?;
        }
        ArchiveType::Zip => {
            let mut tmp = tempfile::tempfile_in(download_dir.path())?;
            io::copy(&mut digest_reader, &mut tmp)?;
            let mut zip = zip::ZipArchive::new(&mut tmp)?;
            zip.extract(download_dir.path())?;
        }
    }
    digest_reader.check(
        download.size,
        download.fingerprint.hash,
        download.url.as_ref(),
    )?;
    fs::rename(download_dir.keep(), dst_dir)?;
    downloaded_path
}

struct DigestReader<'a, D: Digest, R: Read> {
    digest: D,
    reader: R,
    source: &'a str,
    expected_size: u64,
    amount_read: u64,
}

impl<'a, D: Digest, R: Read> DigestReader<'a, D, R> {
    fn new(expected_size: u64, digest: D, reader: R, source: &'a str) -> Self {
        Self {
            digest,
            reader,
            source,
            expected_size,
            amount_read: 0,
        }
    }

    fn check(self, expected_size: u64, expected_hash: &str, source: &str) -> anyhow::Result<()> {
        if self.amount_read != expected_size {
            bail!(
                "Size of {source} was expected to be {expected_size} bytes but was actually \
                {actual_size} bytes.",
                actual_size = self.amount_read
            );
        }
        let actual_hash = hex::encode(self.digest.finalize().as_slice());
        if actual_hash != expected_hash {
            bail!(
                "Fingerprint of {source} did not match:\n\
                Expected: {expected_hash}\n\
                Actual:   {actual_hash}"
            );
        }
        Ok(())
    }
}

impl<'a, D: Digest, R: Read> Read for DigestReader<'a, D, R> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, io::Error> {
        let amount_read = self.reader.read(buffer)?;
        self.amount_read +=
            u64::try_from(amount_read).expect("The pointer size will not be greater than 64 bits.");
        if self.amount_read > self.expected_size {
            return Err(io::Error::new(
                ErrorKind::FileTooLarge,
                format!(
                    "Read {total_read} bytes from {source} but it was expected to be \
                    {expected_size} bytes.",
                    total_read = self.amount_read,
                    source = self.source,
                    expected_size = self.expected_size
                ),
            ));
        }
        self.digest.update(&buffer[0..amount_read]);
        Ok(amount_read)
    }
}

fn install_tools(
    cargo: &Path,
    cargo_binstall: &CargoBinstall,
    tools: &[BinstallTool],
    zig: &Zig,
    install_dirs: &InstallDirs,
    search_path: &OsStr,
) -> anyhow::Result<Vec<FoundTool>> {
    for tool in tools {
        binstall(
            cargo_binstall,
            install_dirs,
            search_path,
            cargo,
            tool.binary_name(),
        )?;
    }

    if let Zig::MissingVersion(version) = zig {
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
            Ok(vec![zig])
        } else {
            bail!(
                "Failed to find zig on PATH={search_path} after installing via \
                `uv tool install --force {zig_requirement}`.",
                search_path = search_path.to_string_lossy()
            )
        }
    } else {
        Ok(Vec::new())
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
            let cargo_binstall = ensure_download(&install_dirs.download_dir, &download)?
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
