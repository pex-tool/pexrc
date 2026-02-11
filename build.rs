// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use anyhow::bail;
use bstr::ByteSlice;
use itertools::Itertools;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
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

#[derive(Deserialize)]
struct RustToolchain<'a> {
    #[serde(borrow)]
    toolchain: Toolchain<'a>,
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
struct Build<'a> {
    #[serde(borrow)]
    zig_version: Cow<'a, str>,
    #[serde(borrow)]
    glibc: Glibc<'a>,
    #[serde(borrow)]
    clib: Clib<'a>,
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
enum Tool {
    #[strum(serialize = "cargo-zigbuild")]
    CargoZigbuild,
    #[strum(serialize = "zig")]
    Zig(String),
    #[strum(serialize = "uv")]
    Uv,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cargo = env::var("CARGO")?;

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
    let tool_install_dir = if let Some(cache_root) = dirs::cache_dir() {
        cache_root.join("pexrc-dev").join("bin")
    } else {
        let cache_dir = target_dir.join(".pexrc-dev").join("bin");
        println!(
            "cargo::warning=Failed to discover the user cache dir; using {cache_dir}",
            cache_dir = cache_dir.display()
        );
        cache_dir
    };

    let mut cmd = Command::new(&cargo);
    cmd.stderr(Stdio::piped());
    cmd.env_remove("CARGO_ENCODED_RUSTFLAGS");
    cmd.env("CARGO_TERM_COLOR", "always");
    for (env_var, path) in ensure_tools_installed(&cargo, &build_config, &tool_install_dir)? {
        cmd.env(env_var, path);
    }
    cmd.args([
        "zigbuild",
        "--package",
        "clib",
        "--profile",
        build_config.clib.profile.as_ref(),
    ]);
    for target in &rust_toolchain.toolchain.targets {
        cmd.arg("--target");
        if target.contains("gnu") {
            cmd.arg(format!(
                "{target}.{glibc_version}",
                target = target.as_ref(),
                glibc_version = build_config.glibc.version(target.as_ref())
            ));
        } else {
            cmd.arg(target.as_ref());
        }
    }

    let child = cmd.spawn()?;
    let result = child.wait_with_output()?;
    if !result.status.success() {
        bail!(
            "Failed to compile clib with exit code {exit_code}:\n{exe} \\\n  {args}\n{output}",
            exit_code = result.status,
            exe = cmd.get_program().to_string_lossy(),
            args = cmd.get_args().map(OsStr::to_string_lossy).join(" \\\n  "),
            output = result.stderr.to_str_lossy()
        );
    }

    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap().into();
    let clibs_dir = out_dir.join("clibs");
    fs::create_dir_all(&clibs_dir)?;
    println!(
        "cargo::rustc-env=CLIBS_DIR={clibs_dir}",
        clibs_dir = clibs_dir.display()
    );

    for target in &rust_toolchain.toolchain.targets {
        let clib_name = if target.contains("-apple-") {
            "libpexrc.dylib"
        } else if target.contains("-pc-windows-") {
            "pexrc.dll"
        } else {
            "libpexrc.so"
        };
        let clib = target_dir
            .join(target.as_ref())
            .join(build_config.clib.profile.as_ref())
            .join(clib_name);
        if !clib.exists() {
            eprintln!(
                "The clib for {target} does not exist at {clib_path}!",
                clib_path = clib.display()
            );
        }
        io::copy(
            &mut File::open(clib)?,
            &mut zstd::Encoder::new(
                File::create(clibs_dir.join(format!("{target}.{clib_name}")))?,
                build_config.clib.compression_level,
            )?,
        )?;
    }

    Ok(())
}

fn ensure_tools_installed(
    cargo: &String,
    build_config: &Build,
    tool_install_dir: &Path,
) -> anyhow::Result<Vec<(OsString, OsString)>> {
    let tool_search_path = if let Some(search_path) =
        env::var_os("PATH").as_deref().map(env::split_paths)
    {
        let search_path = env::join_paths(search_path.chain([PathBuf::from(tool_install_dir)]))?;
        Cow::Owned(search_path.into())
    } else {
        Cow::Borrowed(tool_install_dir.as_os_str())
    };

    let mut missing_tools: Vec<Tool> = Vec::with_capacity(Tool::COUNT);
    let mut found_tools: Vec<(OsString, OsString)> = Vec::with_capacity(Tool::COUNT);
    for tool in Tool::iter() {
        match tool {
            Tool::Zig(_) => {
                if let Some(zig) = find_zig(
                    &["zig", "python-zig"],
                    build_config.zig_version.as_ref(),
                    tool_search_path.as_ref(),
                ) {
                    found_tools.push(zig);
                } else {
                    missing_tools.push(Tool::Zig(build_config.zig_version.to_string()))
                }
            }
            tool => {
                if let Ok(exe) =
                    which_in(tool.as_ref(), Some(&tool_search_path), env::current_dir()?)
                {
                    eprintln!(
                        "Found {tool} at {exe}",
                        tool = tool.as_ref(),
                        exe = exe.display()
                    );
                } else {
                    missing_tools.push(tool)
                }
            }
        }
    }
    if !missing_tools.is_empty() {
        if let Some(value) = env::var_os("PEXRC_INSTALL_TOOLS")
            && value == "1"
        {
            let mut installed_tools =
                install_tools(&cargo, &missing_tools, tool_install_dir, &tool_search_path)?;
            found_tools.append(&mut installed_tools);
        } else {
            bail!(
                "The following tools are required but are not installed: {tools}\n\
                Searched PATH: {search_path}\n\
                Re-run with PEXRC_INSTALL_TOOLS=1 to let the build script install these tools.",
                tools = missing_tools.iter().map(AsRef::as_ref).join(" "),
                search_path = tool_search_path.display()
            );
        }
    }
    Ok(found_tools)
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
    cargo: &str,
    tools: &[Tool],
    install_dir: &Path,
    search_path: &OsStr,
) -> anyhow::Result<Vec<(OsString, OsString)>> {
    if which_in_global("cargo-binstall", Some(search_path)).is_err() {
        let result = Command::new(cargo)
            .args(["install", "--locked", "cargo-binstall"])
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
    let mut zig_version: Option<&str> = None;
    for tool in tools {
        match tool {
            Tool::Zig(version) => zig_version = Some(version.as_str()),
            tool => binstall(cargo, tool.as_ref())?,
        }
    }
    if let Some(zig_version) = zig_version {
        fs::create_dir_all(install_dir)?;
        let zig_requirement = format!("ziglang=={zig_version}");
        let result = Command::new("uv")
            .args(["tool", "install", "--force", &zig_requirement])
            .env("UV_TOOL_BIN_DIR", install_dir.as_os_str())
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

fn binstall(cargo: &str, spec: &str) -> anyhow::Result<()> {
    let result = Command::new(cargo)
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
