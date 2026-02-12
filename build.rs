// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use anyhow::bail;
use bstr::ByteSlice;
use const_format::concatcp;
use itertools::Itertools;
use serde::Deserialize;
use sha2::Digest;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, io};
use strum::{EnumCount, IntoEnumIterator};
use strum_macros::{AsRefStr, EnumCount, EnumIter};
use which::{which_in, which_in_global};

#[derive(Deserialize)]
struct Toolchain<'a> {
    #[serde(borrow)]
    targets: Vec<Cow<'a, str>>,
}

struct GnuLinux<'a> {
    target: Cow<'a, str>,
    zigbuild_target: String,
}

enum Target<'a> {
    Apple(Cow<'a, str>),
    GnuLinux(GnuLinux<'a>),
    Unix(Cow<'a, str>),
    Windows(Cow<'a, str>),
}

impl<'a> Target<'a> {
    fn as_str(&self) -> &str {
        match self {
            Target::Apple(target) | Target::Unix(target) | Target::Windows(target) => {
                target.as_ref()
            }
            Target::GnuLinux(linux) => linux.target.as_ref(),
        }
    }

    fn shared_library_name(&self, lib_name: &str) -> String {
        match self {
            Target::Apple(_) => format!("lib{lib_name}.dylib"),
            Target::GnuLinux(_) | Target::Unix(_) => format!("lib{lib_name}.so"),
            Target::Windows(_) => format!("{lib_name}.dll"),
        }
    }
}

struct ClassifiedTargets<'a> {
    xwin_targets: Vec<Target<'a>>,
    zigbuild_targets: Vec<Target<'a>>,
}

impl<'a> ClassifiedTargets<'a> {
    fn iter_zigbuild_targets(&'a self) -> impl Iterator<Item = &'a str> {
        self.zigbuild_targets.iter().map(|target| {
            if let Target::GnuLinux(gnu_linux) = target {
                gnu_linux.zigbuild_target.as_str()
            } else {
                target.as_str()
            }
        })
    }

    fn iter_xwin_targets(&'a self) -> impl Iterator<Item = &'a str> {
        self.xwin_targets.iter().map(Target::as_str)
    }

    fn iter_targets(&'a self) -> impl Iterator<Item = &'a Target<'a>> {
        self.zigbuild_targets.iter().chain(self.xwin_targets.iter())
    }
}

impl<'a> Toolchain<'a> {
    fn classify(self, glibc: &'a Glibc) -> ClassifiedTargets<'a> {
        let (xwin_targets, zigbuild_targets) = self
            .targets
            .into_iter()
            .map(move |target| {
                if target.contains("-apple-") {
                    Target::Apple(target)
                } else if target.contains("-linux-") {
                    if target.ends_with("-gnu") {
                        let zigbuild_target = format!(
                            "{target}.{glibc_version}",
                            target = target.as_ref(),
                            glibc_version = glibc.version(target.as_ref())
                        );
                        Target::GnuLinux(GnuLinux {
                            target,
                            zigbuild_target,
                        })
                    } else {
                        Target::Unix(target)
                    }
                } else if target.contains("-windows-") {
                    Target::Windows(target)
                } else {
                    panic!("The build system does not know how to handle")
                }
            })
            .partition::<Vec<_>, _>(|target| matches!(target, Target::Windows(_)));
        ClassifiedTargets {
            xwin_targets,
            zigbuild_targets,
        }
    }
}

#[derive(Deserialize)]
struct RustToolchain<'a> {
    #[serde(borrow)]
    toolchain: Toolchain<'a>,
}

#[derive(Deserialize, Debug)]
struct Fingerprint<'a> {
    #[serde(borrow)]
    algorithm: Cow<'a, str>,
    #[serde(borrow)]
    hash: Cow<'a, str>,
}

#[derive(Deserialize, Debug)]
struct DownloadArchive<'a> {
    #[serde(borrow)]
    url: Cow<'a, str>,
    size: u64,
    #[serde(borrow)]
    fingerprint: Fingerprint<'a>,
    #[serde(borrow, default)]
    prefix: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct Glibc<'a> {
    #[serde(borrow)]
    default_version: Cow<'a, str>,
    #[serde(borrow)]
    by_platform: HashMap<Cow<'a, str>, Cow<'a, str>>,
}

impl<'a> Glibc<'a> {
    fn version(&self, target: &str) -> &str {
        self.by_platform
            .get(target)
            .map(|v| v.as_ref())
            .unwrap_or(self.default_version.as_ref())
    }
}

#[derive(Deserialize)]
struct Clib<'a> {
    #[serde(borrow)]
    profile: Cow<'a, str>,
    compression_level: i32,
}

#[derive(Deserialize)]
struct Artifact<'a> {
    #[serde(borrow)]
    target: Cow<'a, str>,
    #[serde(borrow, rename = "type")]
    archive_type: Cow<'a, str>,
    size: u64,
    #[serde(borrow)]
    hash: Cow<'a, str>,
}

#[derive(Deserialize)]
struct CargoBinstall<'a> {
    #[serde(borrow)]
    version: Cow<'a, str>,
    artifacts: Vec<Artifact<'a>>,
}

impl<'a> CargoBinstall<'a> {
    fn download_for(&'a self, target: &'a str) -> anyhow::Result<Option<DownloadArchive<'a>>> {
        for artifact in &self.artifacts {
            if artifact.target != target {
                continue;
            }
            let (algorithm, hash) = if let Some(idx) = artifact.hash.find(":") {
                (
                    &artifact.hash.as_ref()[0..idx],
                    &artifact.hash.as_ref()[idx + 1..],
                )
            } else {
                bail!(
                    "Invalid hash {hash} for cargo-binstall target {target}.\n\
                    Must be of form <algorithm>:<hex digest>",
                    hash = artifact.hash
                );
            };
            return Ok(Some(DownloadArchive {
                url: Cow::Owned(format!(
                    "https://github.com/cargo-bins/cargo-binstall/releases/download/\
                        v{version}/\
                        cargo-binstall-{target}.{ext}",
                    version = self.version,
                    ext = artifact.archive_type
                )),
                size: artifact.size,
                fingerprint: Fingerprint {
                    algorithm: Cow::Borrowed(algorithm),
                    hash: Cow::Borrowed(hash),
                },
                prefix: None,
            }));
        }
        Ok(None)
    }
}

#[derive(Deserialize)]
struct Build<'a> {
    #[serde(borrow)]
    cargo_binstall: CargoBinstall<'a>,
    #[serde(borrow)]
    clib: Clib<'a>,
    #[serde(borrow)]
    glibc: Glibc<'a>,
    #[serde(borrow)]
    mac_osx_sdk: DownloadArchive<'a>,
    #[serde(borrow)]
    zig_version: Cow<'a, str>,
}

#[derive(Deserialize)]
struct Metadata<'a> {
    #[serde(borrow)]
    build: Build<'a>,
}

#[derive(Deserialize)]
struct Package<'a> {
    #[serde(borrow)]
    metadata: Metadata<'a>,
}

#[derive(Deserialize)]
struct CargoManifest<'a> {
    #[serde(borrow)]
    package: Package<'a>,
}

#[derive(AsRefStr, EnumCount, EnumIter)]
enum BinstallTool {
    #[strum(serialize = "cargo-xwin")]
    CargoXwin,
    #[strum(serialize = "cargo-zigbuild")]
    CargoZigbuild,
    #[strum(serialize = "uv")]
    Uv,
}

struct ToolBox<'a> {
    binstall: &'a CargoBinstall<'a>,
    zig_version: &'a str,
    binstall_tools: Vec<BinstallTool>,
    downloads: Vec<(&'static str, &'a DownloadArchive<'a>)>,
}

impl<'a> ToolBox<'a> {
    fn collect_tools(build: &'a Build) -> ToolBox<'a> {
        ToolBox {
            binstall: &build.cargo_binstall,
            zig_version: build.zig_version.as_ref(),
            binstall_tools: BinstallTool::iter().collect::<Vec<_>>(),
            downloads: vec![("SDKROOT", &build.mac_osx_sdk)],
        }
    }
}

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
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cargo: PathBuf = env::var("CARGO")?.into();

    let data = fs::read_to_string("rust-toolchain")?;
    let rust_toolchain: RustToolchain = toml::from_str(data.as_str())?;

    let data = {
        let manifest_path = env::var("CARGO_MANIFEST_PATH")?;
        fs::read_to_string(manifest_path)?
    };
    let build_config = {
        let cargo_manifest: CargoManifest = toml::from_str(data.as_str())?;
        cargo_manifest.package.metadata.build
    };
    let target_dir: PathBuf = if let Some(custom_target_dir) = env::var_os("CARGO_TARGET_DIR") {
        custom_target_dir.into()
    } else {
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("target")
    };
    let mut install_dirs = if let Some(cache_dir) = dirs::cache_dir() {
        InstallDirs::new(cache_dir.join("pexrc-dev"))
    } else {
        let cache_dir = target_dir.join(".pexrc-dev");
        println!(
            "cargo::warning=Failed to discover the user cache dir; using {cache_dir}",
            cache_dir = cache_dir.display()
        );
        InstallDirs::new(cache_dir)
    };

    let targets = rust_toolchain.toolchain.classify(&build_config.glibc);
    custom_cargo_build(
        &cargo,
        ["xwin", "build"],
        &build_config,
        &mut install_dirs,
        targets.iter_xwin_targets(),
    )?;
    custom_cargo_build(
        &cargo,
        ["zigbuild"],
        &build_config,
        &mut install_dirs,
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
            .join(build_config.clib.profile.as_ref())
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
                build_config.clib.compression_level,
            )?,
        )?;
    }

    Ok(())
}

fn custom_cargo_build<'a>(
    cargo: &Path,
    custom_build_args: impl IntoIterator<Item = &'a str>,
    build_config: &Build,
    install_dirs: &mut InstallDirs,
    targets: impl Iterator<Item = &'a str>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new(cargo);
    let cmd = cmd
        .stderr(Stdio::piped())
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TERM_COLOR", "always");
    for (env_var, path) in ensure_tools_installed(cargo, build_config, install_dirs)? {
        println!(
            "cargo::rustc-env={env_var}={path}",
            env_var = env_var.display(),
            path = path.display()
        );
        cmd.env(env_var, path);
    }
    cmd.args(custom_build_args).args([
        "--package",
        "clib",
        "--profile",
        build_config.clib.profile.as_ref(),
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
    build_config: &Build,
    install_dirs: &InstallDirs,
) -> anyhow::Result<Vec<(OsString, OsString)>> {
    let tool_search_path =
        if let Some(search_path) = env::var_os("PATH").as_deref().map(env::split_paths) {
            let search_path = env::join_paths(search_path.chain([install_dirs.bin_dir.clone()]))?;
            Cow::Owned(search_path)
        } else {
            Cow::Borrowed(install_dirs.bin_dir.as_os_str())
        };

    let toolbox = ToolBox::collect_tools(build_config);
    let mut found_tools: Vec<(OsString, OsString)> =
        Vec::with_capacity(BinstallTool::COUNT + toolbox.downloads.len());
    let mut missing_binstall_tools: Vec<BinstallTool> = Vec::with_capacity(BinstallTool::COUNT);
    let mut missing_zig: Option<&str> = None;
    if let Some(zig) = find_zig(
        &["zig", "python-zig"],
        toolbox.zig_version,
        tool_search_path.as_ref(),
    ) {
        found_tools.push(zig);
    } else {
        missing_zig = Some(toolbox.zig_version);
    }
    for tool in toolbox.binstall_tools {
        if let Ok(exe) = which_in(tool.as_ref(), Some(&tool_search_path), env::current_dir()?) {
            eprintln!(
                "Found {tool} at {exe}",
                tool = tool.as_ref(),
                exe = exe.display()
            );
        } else {
            missing_binstall_tools.push(tool)
        }
    }
    if !missing_binstall_tools.is_empty() || missing_zig.is_some() {
        if let Some(value) = env::var_os("PEXRC_INSTALL_TOOLS")
            && value == "1"
        {
            let mut installed_tools = install_tools(
                cargo,
                toolbox.binstall,
                missing_binstall_tools.as_slice(),
                missing_zig,
                install_dirs,
                &tool_search_path,
            )?;
            found_tools.append(&mut installed_tools);
        } else {
            bail!(
                "The following tools are required but are not installed: {tools}\n\
                Searched PATH: {search_path}\n\
                Re-run with PEXRC_INSTALL_TOOLS=1 to let the build script install these tools.",
                tools = missing_binstall_tools
                    .iter()
                    .map(|tool| tool.as_ref().to_owned())
                    .chain(missing_zig.iter().map(|version| format!("zig@{version}")))
                    .join(" "),
                search_path = tool_search_path.display()
            );
        }
    }
    for (env_var, download_path) in ensure_downloads(toolbox.downloads, install_dirs)? {
        found_tools.push((env_var, download_path))
    }
    Ok(found_tools)
}

fn ensure_downloads<'a>(
    downloads: impl IntoIterator<
        IntoIter = impl ExactSizeIterator<Item = (&'static str, &'a DownloadArchive<'a>)>,
    >,
    install_dirs: &'a InstallDirs,
) -> anyhow::Result<Vec<(OsString, OsString)>> {
    let downloads = downloads.into_iter();
    if downloads.len() == 0 {
        return Ok(Vec::new());
    }

    let mut download_paths: Vec<(OsString, OsString)> = Vec::with_capacity(downloads.len());
    for (env_var, download) in downloads {
        let download_path = ensure_download(&install_dirs.download_dir, download)?;
        download_paths.push((OsString::from(env_var), download_path.into_os_string()));
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
    let dst_dir = download_dir.join(download.fingerprint.hash.as_ref());
    let downloaded_path = Ok(if let Some(prefix) = download.prefix.as_ref() {
        dst_dir.join(prefix.as_ref())
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

    let hasher = match download.fingerprint.algorithm.as_ref() {
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
        download.fingerprint.hash.as_ref(),
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

fn find_zig(
    binary_names: &[&str],
    version: &str,
    search_path: &OsStr,
) -> Option<(OsString, OsString)> {
    for binary_name in binary_names {
        if let Ok(zig_paths) = which_in_global(binary_name, Some(search_path)) {
            for zig in zig_paths {
                if let Some(zig_version) = get_zig_version(&zig)
                    && zig_version == version
                {
                    return Some(("CARGO_ZIGBUILD_ZIG_PATH".into(), zig.into_os_string()));
                }
            }
        }
    }
    None
}

fn get_zig_version(zig: impl AsRef<Path>) -> Option<String> {
    Command::new(zig.as_ref())
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

fn install_tools(
    cargo: &Path,
    cargo_binstall: &CargoBinstall,
    tools: &[BinstallTool],
    zig: Option<&str>,
    install_dirs: &InstallDirs,
    search_path: &OsStr,
) -> anyhow::Result<Vec<(OsString, OsString)>> {
    for tool in tools {
        binstall(
            cargo_binstall,
            install_dirs,
            search_path,
            cargo,
            tool.as_ref(),
        )?;
    }

    if let Some(zig_version) = zig {
        let zig_requirement = format!("ziglang=={zig_version}");
        fs::create_dir_all(&install_dirs.bin_dir)?;
        let result = Command::new("uv")
            .args(["tool", "install", "--force", &zig_requirement])
            .env("UV_TOOL_BIN_DIR", install_dirs.bin_dir.as_os_str())
            .stderr(Stdio::piped())
            .spawn()?
            .wait_with_output()?;
        if !result.status.success() {
            bail!(
                "Failed to install zig {zig_version} via `uv tool install {zig_requirement}`:\n\
                {stderr}",
                stderr = result.stderr.to_str_lossy()
            )
        } else if let Some(zig) = find_zig(&["python-zig"], zig_version, search_path) {
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
    let exes = which_in_global("cargo-binstall", Some(search_path))?.collect::<Vec<_>>();
    if !exes.is_empty() {
        eprintln!("Found cargo-binstall at:");
        for (idx, exe) in exes.iter().enumerate() {
            eprintln!("{idx} {exe}", idx = idx + 1, exe = exe.display());
        }
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
