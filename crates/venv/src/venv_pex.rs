// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::io;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use fs_err as fs;
use fs_err::File;
use indexmap::{IndexMap, IndexSet};
use log::warn;
use logging_timer::time;
use pex::{BinPath, InheritPath, Layout, Pex, PexInfo};
use platform::{mark_executable, path_as_bytes, path_as_str, symlink_or_link_or_copy};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use scripts::{Scripts, VenvPex, VenvPexRepl};
use zip::ZipArchive;

use crate::virtualenv::Virtualenv;

fn populate_from_loose_pex<'a>(
    venv: &Virtualenv,
    loose_pex: &'a Pex<'a>,
    resolved_wheels: IndexSet<&'a str>,
    populate_pex_info: bool,
) -> anyhow::Result<()> {
    let site_packages_path = venv.site_packages_path();
    collect_wheels_from_directory_pex(loose_pex, resolved_wheels)?
        .into_par_iter()
        .try_for_each(|wheel_dir| populate_wheel_dir(&wheel_dir, &site_packages_path))?;
    populate_user_code_from_directory_pex(
        loose_pex,
        venv.prefix(),
        &site_packages_path,
        populate_pex_info,
    )?;
    Ok(())
}

fn collect_wheels_from_directory_pex(
    pex: &Pex,
    resolved_wheels: IndexSet<&str>,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut wheels = Vec::with_capacity(resolved_wheels.len());
    let deps_dir = pex.path.join(".deps");
    if deps_dir.is_dir() {
        for entry in fs::read_dir(deps_dir)? {
            let entry = entry?;
            if let Ok(wheel_file_name) = platform::os_str_as_str(&entry.file_name())
                && resolved_wheels.contains(wheel_file_name)
            {
                wheels.push(entry.path())
            }
        }
    }
    Ok(wheels)
}

fn populate_wheel_dir(wheel: &Path, site_packages_path: &Path) -> anyhow::Result<()> {
    let user_code = walkdir::WalkDir::new(wheel)
        .min_depth(1)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    user_code.into_par_iter().try_for_each(|entry| {
        let dst_path = site_packages_path.join(entry.path().strip_prefix(wheel).expect(
            "Walked unpacked wheel paths should be child paths of the unpacked wheel root dir.",
        ));
        if entry.path().is_dir() {
            fs::create_dir_all(&dst_path)
        } else {
            if let Some(parent_dir) = dst_path.parent() {
                fs::create_dir_all(parent_dir)?;
            }
            match File::create_new(&dst_path) {
                Ok(mut dst_file) => {
                    let mut src = File::open(entry.path())?;
                    io::copy(&mut src, &mut dst_file)?;
                }
                Err(_) => {
                    // TODO: Track provenance.
                    warn!("Collision for {dst_path}", dst_path = dst_path.display());
                }
            }
            Ok(())
        }
    })?;
    Ok(())
}

fn populate_from_packed_pex<'a>(
    venv: &Virtualenv,
    packed_pex: &'a Pex<'a>,
    resolved_wheels: IndexSet<&'a str>,
    populate_pex_info: bool,
) -> anyhow::Result<()> {
    let site_packages_path = venv.site_packages_path();
    collect_wheels_from_directory_pex(packed_pex, resolved_wheels)?
        .into_par_iter()
        .try_for_each(|wheel_zip| populate_whl(&wheel_zip, &site_packages_path))?;
    populate_user_code_from_directory_pex(
        packed_pex,
        venv.prefix(),
        &site_packages_path,
        populate_pex_info,
    )?;
    Ok(())
}

fn populate_whl(wheel: &Path, site_packages_path: &Path) -> anyhow::Result<()> {
    populate_whl_zip(
        wheel,
        ZipArchive::new(File::open(wheel)?.into_file())?,
        site_packages_path,
    )
}

fn populate_whl_zip<P: AsRef<Path> + Send + Sync>(
    wheel: P,
    whl_zip: ZipArchive<std::fs::File>,
    site_packages_path: &Path,
) -> anyhow::Result<()> {
    let metadata = whl_zip.metadata();
    (0..whl_zip.len()).into_par_iter().try_for_each(|index| {
        let zip_fp = File::open(wheel.as_ref())?;
        let mut zip = unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
        extract_idx(site_packages_path, index, &mut zip)?;
        Ok(())
    })
}

fn populate_user_code_from_directory_pex<'a>(
    directory_pex: &'a Pex<'a>,
    venv_dir: &Path,
    site_packages_path: &Path,
    populate_pex_info: bool,
) -> anyhow::Result<()> {
    let excludes: HashSet<PathBuf> = [
        ".deps",
        "PEX-INFO",
        "__main__.py",
        "__pex__",
        "__pycache__",
        "pex",
        "pex-repl",
    ]
    .into_iter()
    .map(|rel_path| directory_pex.path.join(rel_path))
    .collect();
    let _pex_info_exclude = if populate_pex_info {
        None
    } else {
        Some(directory_pex.path.join("PEX-INFO"))
    };
    let user_code = walkdir::WalkDir::new(directory_pex.path)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !excludes.contains(entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    user_code.into_par_iter().try_for_each(|entry| {
        let dst_path =
            site_packages_path.join(entry.path().strip_prefix(directory_pex.path).expect(
                "Walked directory PEX paths should be child paths of the directory PEX root dir.",
            ));
        if entry.file_type().is_dir() {
            fs::create_dir_all(dst_path)
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), dst_path).map(|_| ())
        }
    })?;

    if populate_pex_info {
        fs::copy(
            directory_pex.path.join("PEX-INFO"),
            venv_dir.join("PEX-INFO"),
        )?;
    }
    Ok(())
}

fn populate_from_zip_app_with_whl_deps<'a>(
    venv: &Virtualenv,
    zip_app_pex: &'a Pex<'a>,
    resolved_wheels: IndexSet<&'a str>,
    populate_pex_info: bool,
) -> anyhow::Result<()> {
    let pex_zip = ZipArchive::new(File::open(zip_app_pex.path)?)?;
    let metadata = pex_zip.metadata();
    let extract_indexes = pex_zip
        .file_names()
        .enumerate()
        .filter_map(|(idx, name)| {
            if [".deps/", "__pex__/"]
                .iter()
                .any(|exclude_dir| name.starts_with(exclude_dir))
                || ["PEX-INFO", "__main__.py"].contains(&name)
            {
                None
            } else {
                Some(idx)
            }
        })
        .collect::<Vec<_>>();
    let site_packages_path = venv.site_packages_path();
    extract_indexes
        .into_par_iter()
        .try_for_each(|index| -> anyhow::Result<()> {
            let zip_fp = File::open(zip_app_pex.path)?;
            let mut zip = unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
            extract_idx(&site_packages_path, index, &mut zip)?;
            Ok(())
        })?;

    let wheel_file_names = resolved_wheels.into_iter().collect::<Vec<_>>();
    wheel_file_names
        .into_par_iter()
        .try_for_each(|wheel_file_name| {
            let zip_fp = File::open(zip_app_pex.path)?;
            let mut zip = unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
            let whl_file = zip.by_name_seek(&[".deps", wheel_file_name].join("/"))?;
            let whl_zip = ZipArchive::new(whl_file)?;
            let whl_zip_metadata = whl_zip.metadata();
            (0..whl_zip.len()).into_par_iter().try_for_each(|index| {
                let zip_fp = File::open(zip_app_pex.path)?;
                let mut zip =
                    unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
                let whl_file = zip.by_name_seek(&[".deps", wheel_file_name].join("/"))?;
                let mut whl_zip = unsafe {
                    ZipArchive::unsafe_new_with_metadata(whl_file, whl_zip_metadata.clone())
                };
                extract_idx(&site_packages_path, index, &mut whl_zip)
            })
        })?;

    if populate_pex_info {
        let mut pex_zip = ZipArchive::new(File::open(zip_app_pex.path)?)?;
        let mut pex_info_src_fp = pex_zip.by_name("PEX-INFO")?;
        let mut pex_info_dst_fp = File::create_new(venv.prefix().join("PEX-INFO"))?;
        io::copy(&mut pex_info_src_fp, &mut pex_info_dst_fp)?;
    }
    Ok(())
}

fn populate_from_zip_app<'a>(
    venv: &Virtualenv,
    zip_app_pex: &'a Pex<'a>,
    resolved_wheels: IndexSet<&'a str>,
    populate_pex_info: bool,
) -> anyhow::Result<()> {
    let mut pex_zip = ZipArchive::new(File::open(zip_app_pex.path)?)?;
    let metadata = pex_zip.metadata();
    let extract_indexes = pex_zip
        .file_names()
        .enumerate()
        .filter_map(|(idx, name)| {
            // TODO: XXX: Deal with .layout and .prefix/ in wheel chroots.
            if name.starts_with("__pex__/")
                || [".deps/", "PEX-INFO", "__main__.py"].contains(&name)
                || name.starts_with(".deps/")
                    && name[6..]
                        .split("/")
                        .next()
                        .map(|whl_name| !resolved_wheels.contains(whl_name))
                        .unwrap_or(true)
            {
                None
            } else {
                Some(idx)
            }
        })
        .collect::<Vec<_>>();
    let site_packages_path = venv.site_packages_path();
    extract_indexes
        .into_par_iter()
        .try_for_each(|index| -> anyhow::Result<()> {
            let zip_fp = File::open(zip_app_pex.path)?;
            let mut zip = unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
            extract_idx(&site_packages_path, index, &mut zip)?;
            Ok(())
        })?;
    if populate_pex_info {
        let mut pex_info_src_fp = pex_zip.by_name("PEX-INFO")?;
        let mut pex_info_dst_fp = File::create_new(venv.prefix().join("PEX-INFO"))?;
        io::copy(&mut pex_info_src_fp, &mut pex_info_dst_fp)?;
    }
    Ok(())
}

#[time("debug", "{}")]
pub fn populate_user_code_and_wheels<'a>(
    venv: &Virtualenv,
    pex: &'a Pex<'a>,
    resolved_wheels: IndexSet<&'a str>,
    populate_pex_info: bool,
) -> anyhow::Result<()> {
    match pex.layout {
        Layout::Loose => populate_from_loose_pex(venv, pex, resolved_wheels, populate_pex_info),
        Layout::Packed => populate_from_packed_pex(venv, pex, resolved_wheels, populate_pex_info),
        Layout::ZipApp => {
            if pex.info.deps_are_wheel_files {
                populate_from_zip_app_with_whl_deps(venv, pex, resolved_wheels, populate_pex_info)
            } else {
                populate_from_zip_app(venv, pex, resolved_wheels, populate_pex_info)
            }
        }
    }
}

#[time("debug", "{}")]
pub fn populate<'a>(
    venv: &Virtualenv,
    resting_venv_dir: &Path,
    pex: &'a Pex<'a>,
    resolved_wheels: IndexSet<&'a str>,
    scripts: &mut Scripts,
) -> anyhow::Result<()> {
    let selected_wheels = resolved_wheels.iter().copied().collect::<Vec<_>>();
    populate_user_code_and_wheels(venv, pex, resolved_wheels, true)?;

    let interpreter_relpath = venv
        .interpreter
        .path
        .strip_prefix(&venv.interpreter.prefix)?;
    let shebang_interpreter = resting_venv_dir.join(interpreter_relpath);
    let shebang_arg = if (pex.info.venv && pex.info.venv_hermetic_scripts)
        || (!pex.info.venv
            && pex.info.inherit_path.unwrap_or(InheritPath::False) == InheritPath::False)
    {
        Some(venv.interpreter.hermetic_args())
    } else {
        None
    };
    write_main(venv, &shebang_interpreter, shebang_arg, &pex.info, scripts)?;
    write_repl(
        venv,
        &shebang_interpreter,
        shebang_arg,
        pex.path,
        &pex.info,
        selected_wheels,
        scripts,
    )
}

fn extract_idx<R>(
    dst_dir: impl AsRef<Path>,
    index: usize,
    zip: &mut ZipArchive<R>,
) -> anyhow::Result<()>
where
    R: Read + Seek,
{
    let mut zip_file = zip.by_index(index)?;
    let dst_path =
        dst_dir
            .as_ref()
            .join(if zip_file.name().starts_with(".deps/") {
                zip_file.name().splitn(3, "/").nth(2).ok_or_else(|| {
                    anyhow!("Invalid PEX .deps/ entry {name}", name = zip_file.name())
                })?
            } else {
                zip_file.name()
            });
    if zip_file.is_dir() {
        fs::create_dir_all(dst_path)?;
    } else {
        if let Some(parent_dir) = dst_path.parent() {
            fs::create_dir_all(parent_dir)?;
        }
        match File::create_new(&dst_path) {
            Ok(mut dst_file) => {
                io::copy(&mut zip_file, &mut dst_file)?;
            }
            Err(_) => {
                // TODO: Track provenance.
                warn!("Collision for {dst_path}", dst_path = dst_path.display());
            }
        }
    }
    Ok(())
}

fn write_shebang_bytes(
    file: &mut File,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
) -> anyhow::Result<()> {
    file.write_all(b"#!")?;
    file.write_all(path_as_bytes(shebang_interpreter)?)?;
    if let Some(shebang_arg) = shebang_arg {
        file.write_all(b" ")?;
        file.write_all(shebang_arg.as_bytes())?;
    }
    file.write_all(b"\n")?;
    Ok(())
}

fn as_python_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

struct OptionalPythonStr<'a>(Option<&'a str>);

impl<'a> Display for OptionalPythonStr<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = self.0 {
            write!(f, "r\"{value}\"")
        } else {
            f.write_str("None")
        }
    }
}

struct PythonListStr<'a>(&'a Vec<String>);

impl<'a> Display for PythonListStr<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for (idx, item) in self.0.iter().enumerate() {
            write!(f, "r\"{item}\"")?;
            if idx < self.0.len() - 1 {
                write!(f, ",")?;
            }
        }
        write!(f, "]")
    }
}

struct PythonListTupleStrStr<'a>(&'a IndexMap<String, String>);

impl<'a> Display for PythonListTupleStrStr<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for (idx, (item1, item2)) in self.0.iter().enumerate() {
            write!(f, "(r\"{item1}\",r\"{item2}\")")?;
            if idx < self.0.len() - 1 {
                write!(f, ",")?;
            }
        }
        write!(f, "]")
    }
}

fn write_main(
    venv: &Virtualenv,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    pex_info: &PexInfo,
    scripts: &mut Scripts,
) -> anyhow::Result<()> {
    let main_py = venv.prefix().join("__main__.py");
    let mut main_py_fp = File::create_new(&main_py)?;
    write_shebang_bytes(&mut main_py_fp, shebang_interpreter, shebang_arg)?;
    let venv_pex_script = VenvPex::read(scripts)?;
    main_py_fp.write_all(venv_pex_script.contents().as_bytes())?;

    write!(
        main_py_fp,
        "{}",
        format_args!(
            r#"

if __name__ == "__main__":
    boot(
        shebang_python=r"{shebang_python}",
        venv_bin_dir=r"{venv_bin_dir}",
        bin_path=r"{bin_path}",
        strip_pex_env={strip_pex_env},
        bind_resource_paths={bind_resource_paths},
        inject_env={inject_env},
        inject_args={inject_args},
        entry_point={entry_point},
        script={script},
        hermetic_re_exec={hermetic_re_exec},
    )
"#,
            shebang_python = path_as_str(shebang_interpreter)?,
            venv_bin_dir = venv.bin_dir_relpath,
            bin_path = pex_info
                .venv_bin_path
                .as_ref()
                .unwrap_or(&BinPath::False)
                .as_str(),
            strip_pex_env = as_python_bool(pex_info.strip_pex_env.unwrap_or(true)),
            bind_resource_paths = PythonListTupleStrStr(&pex_info.bind_resource_paths),
            inject_env = PythonListTupleStrStr(&pex_info.inject_env),
            inject_args = PythonListStr(&pex_info.inject_args),
            entry_point = OptionalPythonStr(pex_info.entry_point.as_deref()),
            script = OptionalPythonStr(pex_info.script.as_deref()),
            hermetic_re_exec = OptionalPythonStr(if pex_info.venv_hermetic_scripts {
                Some(venv.interpreter.hermetic_args())
            } else {
                None
            })
        )
    )?;
    mark_executable(main_py_fp.file_mut())?;
    Ok(symlink_or_link_or_copy(
        &main_py,
        venv.prefix().join("pex"),
        true,
    )?)
}

fn write_repl(
    venv: &Virtualenv,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    pex: &Path,
    pex_info: &PexInfo,
    selected_wheels: Vec<&str>,
    scripts: &mut Scripts,
) -> anyhow::Result<()> {
    let mut pex_repl_py_fp = File::create_new(venv.prefix().join("pex-repl"))?;
    write_shebang_bytes(&mut pex_repl_py_fp, shebang_interpreter, shebang_arg)?;
    let venv_pex_repl_script = VenvPexRepl::read(scripts)?;
    pex_repl_py_fp.write_all(venv_pex_repl_script.contents().as_bytes())?;

    let activation_summary = if selected_wheels.is_empty() {
        format_args!("")
    } else {
        format_args!(
            "{req_count} {requirements} and {dist_count} activated {distributions}",
            req_count = pex_info.requirements.len(),
            requirements = if pex_info.requirements.len() == 1 {
                "requirement"
            } else {
                "requirements"
            },
            dist_count = selected_wheels.len(),
            distributions = if selected_wheels.len() == 1 {
                "distribution"
            } else {
                "distributions"
            }
        )
    };

    struct ActivationDetails<'a> {
        requirements: &'a Vec<String>,
        selected_wheels: &'a Vec<&'a str>,
    }

    impl<'a> Display for ActivationDetails<'a> {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            if !self.requirements.is_empty() {
                writeln!(f, "Requirements:")?;
                for requirement in self.requirements {
                    writeln!(f, "  {requirement}")?;
                }
                writeln!(f, "Activated Distributions:")?;
                for selected_wheel in self.selected_wheels {
                    writeln!(f, "  {selected_wheel}")?;
                }
            }
            Ok(())
        }
    }

    write!(
        pex_repl_py_fp,
        "{}",
        format_args!(
            r#"


_PS1 = "{ps1}"
_PS2 = "{ps2}"
_PEX_VERSION = "{pex_version}"
_SEED_PEX = r"{seed_pex}"
_ACTIVATION_SUMMARY = "{activation_summary}"
_ACTIVATION_DETAILS = """{activation_details}"""


if __name__ == "__main__":
    import os

    _create_pex_repl(
        ps1=_PS1,
        ps2=_PS2,
        pex_version=_PEX_VERSION,
        pex_info=os.path.join(os.path.dirname(__file__), "PEX-INFO"),
        seed_pex=_SEED_PEX,
        activation_summary=_ACTIVATION_SUMMARY,
        activation_details=_ACTIVATION_DETAILS,
        history=os.environ.get("PEX_INTERPRETER_HISTORY", "0").lower() in ("1", "true"),
        history_file=os.environ.get("PEX_INTERPRETER_HISTORY_FILE")
    )()
"#,
            ps1 = ">>>",
            ps2 = "...",
            pex_version = pex_info
                .build_properties
                .get("pex_version")
                .map(String::as_ref)
                .unwrap_or("(unknown version)"),
            seed_pex = path_as_str(pex)?,
            activation_summary = activation_summary,
            activation_details = ActivationDetails {
                requirements: &pex_info.requirements,
                selected_wheels: &selected_wheels
            },
        )
    )?;
    mark_executable(pex_repl_py_fp.file_mut())?;

    Ok(())
}
