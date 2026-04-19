// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::io;
use std::io::{BufReader, ErrorKind, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail};
use cache::{Fingerprint, default_digest, fingerprint_file};
use fs_err as fs;
use fs_err::File;
use indexmap::IndexMap;
use ini::{Ini, Properties};
use log::warn;
use logging_timer::time;
use pex::{BinPath, Layout, Pex, PexInfo, ResolvedWheel};
use platform::{mark_executable, path_as_bytes, path_as_str, symlink_or_link_or_copy};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use scripts::{Scripts, VenvPex, VenvPexRepl};
use zip::ZipArchive;

use crate::Provenance;
use crate::virtualenv::Virtualenv;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum InstallScope {
    All,
    Deps,
    Srcs,
}

impl InstallScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallScope::All => "all",
            InstallScope::Deps => "deps",
            InstallScope::Srcs => "srcs",
        }
    }
}

fn populate_from_loose_pex<'a>(
    venv: &Virtualenv,
    loose_pex: &'a Pex<'a>,
    resolved_wheels: &IndexMap<&'a str, ResolvedWheel<'a>>,
    populate_pex_info: bool,
    scope: InstallScope,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let site_packages_path = venv.site_packages_path();
    if matches!(scope, InstallScope::All | InstallScope::Deps) {
        collect_wheels_from_directory_pex(loose_pex, resolved_wheels)?
            .into_par_iter()
            .try_for_each(|wheel_dir| {
                populate_wheel_dir(&wheel_dir, &site_packages_path, provenance.clone())
            })?;
    }
    if matches!(scope, InstallScope::All | InstallScope::Srcs) {
        populate_user_code_from_directory_pex(
            loose_pex,
            venv,
            &site_packages_path,
            populate_pex_info,
            provenance,
        )?;
    }
    Ok(())
}

fn collect_wheels_from_directory_pex(
    pex: &Pex,
    resolved_wheels: &IndexMap<&str, ResolvedWheel>,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut wheels = Vec::with_capacity(resolved_wheels.len());
    let deps_dir = pex.path.join(".deps");
    if deps_dir.is_dir() {
        for entry in fs::read_dir(deps_dir)? {
            let entry = entry?;
            if let Ok(wheel_file_name) = platform::os_str_as_str(&entry.file_name())
                && resolved_wheels.contains_key(wheel_file_name)
            {
                wheels.push(entry.path())
            }
        }
    }
    Ok(wheels)
}

fn spread(
    pex: &Pex,
    wheel: ResolvedWheel,
    virtualenv: &Virtualenv,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let entry_points = virtualenv
        .site_packages_path()
        .join(wheel.dist_info_dir())
        .join("entry_points.txt");
    if entry_points.exists() {
        install_scripts(
            pex,
            &entry_points,
            virtualenv,
            shebang_interpreter,
            shebang_arg,
            provenance,
        )?;
    }
    Ok(())
}

fn install_scripts(
    pex: &Pex,
    entry_points_txt: &Path,
    virtualenv: &Virtualenv,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let entry_points = Ini::load_from_file(entry_points_txt)?;
    if let Some(console_scripts) = entry_points.section(Some("console_scripts"))
        && !console_scripts.is_empty()
    {
        let script_dir = virtualenv.prefix().join(virtualenv.bin_dir_relpath);
        for (name, entry_point) in console_scripts {
            create_script(
                pex,
                shebang_interpreter,
                shebang_arg,
                name,
                entry_point,
                &script_dir,
                false,
                provenance.clone(),
            )?;
        }
    }
    if let Some(gui_scripts) = entry_points.section(Some("gui_scripts"))
        && !gui_scripts.is_empty()
    {
        struct GuiScriptsList<'a>(&'a Properties);
        impl<'a> Display for GuiScriptsList<'a> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                for (name, _) in self.0 {
                    writeln!(f, "{name}")?;
                }
                Ok(())
            }
        }
        warn!(
            "There is currently no support for gui scripts, skipping install of {count}.\n\
            Found these installing {entry_points_txt} in venv at {venv}:\n\
            {gui_scripts_list}",
            count = gui_scripts.len(),
            entry_points_txt = entry_points_txt.display(),
            venv = virtualenv.prefix().display(),
            gui_scripts_list = GuiScriptsList(gui_scripts)
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[cfg(unix)]
fn create_script(
    _pex: &Pex,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    name: &str,
    entry_point: &str,
    dest_dir: &Path,
    gui: bool,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    assert!(!gui, "There is no support for gui scripts yet.");

    let script_path = dest_dir.join(name);
    let script_contents = create_script_contents(shebang_interpreter, shebang_arg, entry_point)?;
    let mut script_file = match File::create_new(&script_path) {
        Ok(script_file) => {
            provenance.record(entry_point, script_path);
            script_file
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            let fingerprint = Fingerprint::try_from(BufReader::new(script_contents.as_bytes()))?;
            provenance.record_collision(
                entry_point,
                fingerprint,
                script_contents.len(),
                script_path,
            );
            return Ok(());
        }
        Err(err) => bail!("{err}"),
    };
    script_file.write_all(script_contents.as_bytes())?;
    mark_executable(script_file.file_mut())?;
    Ok(())
}

#[cfg(windows)]
fn create_script(
    pex: &Pex,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    name: &str,
    entry_point: &str,
    dest_dir: &Path,
    gui: bool,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    assert!(!gui, "There is no support for gui scripts yet.");

    let script_path = dest_dir.join(name).with_extension("exe");
    let script_contents = create_script_contents(shebang_interpreter, shebang_arg, entry_point)?;
    let mut script_file = match File::create_new(&script_path) {
        Ok(script_file) => {
            provenance.record(entry_point, script_path);
            script_file
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            provenance.record_collision(entry_point, script_path);
            return Ok(());
        }
        Err(err) => bail!("{err}"),
    };
    python_proxy::create(
        python_proxy::ProxySource::Pex(pex),
        shebang_interpreter,
        script_file.into_file(),
        Some(script_contents),
    )
}

fn create_script_contents(
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    entry_point: &str,
) -> anyhow::Result<String> {
    struct RenderShebang<'a>(&'a str, Option<&'a str>);
    impl<'a> Display for RenderShebang<'a> {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "#!{interpreter}", interpreter = self.0)?;
            if let Some(args) = self.1 {
                write!(f, " {args}")?;
            }
            Ok(())
        }
    }
    let shebang = RenderShebang(path_as_str(shebang_interpreter)?, shebang_arg);

    let mut components = entry_point.splitn(2, ":");
    let modname = components
        .next()
        .expect("A split always yield at least one item.");
    if let Some(attrs) = components.next()
        && !attrs.is_empty()
    {
        struct RenderAttrsTuple<'a>(Vec<&'a str>);
        impl<'a> Display for RenderAttrsTuple<'a> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "(")?;
                for attr in &self.0 {
                    write!(f, "\"{attr}\",")?;
                }
                write!(f, ")")?;
                Ok(())
            }
        }

        Ok(format!(
            r##"{shebang}
# -*- coding: utf-8 -*-
import importlib
import sys

entry_point = importlib.import_module("{modname}")
for attr in {attrs_tuple}:
    entry_point = getattr(entry_point, attr)

if __name__ == "__main__":
    sys.exit(entry_point())
"##,
            attrs_tuple = RenderAttrsTuple(attrs.split(".").collect())
        ))
    } else {
        Ok(format!(
            r##"{shebang}
# -*- coding: utf-8 -*-
import runpy
import sys

if __name__ == "__main__":
    runpy.run_module("{modname}", run_name="__main__", alter_sys=True)
    sys.exit(0)
"##,
        ))
    }
}

fn populate_wheel_dir(
    wheel: &Path,
    site_packages_path: &Path,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let wheel_contents = walkdir::WalkDir::new(wheel)
        .min_depth(1)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    wheel_contents.into_par_iter().try_for_each(|entry| {
        let dst_path = site_packages_path.join(entry.path().strip_prefix(wheel).expect(
            "Walked unpacked wheel paths should be child paths of the unpacked wheel root dir.",
        ));
        if entry.path().is_dir() {
            fs::create_dir_all(&dst_path)?;
        } else {
            if let Some(parent_dir) = dst_path.parent() {
                fs::create_dir_all(parent_dir)?;
            }
            match File::create_new(&dst_path) {
                Ok(mut dst_file) => {
                    provenance.record(entry.path().display(), dst_path);
                    let mut src = File::open(entry.path())?;
                    io::copy(&mut src, &mut dst_file)?;
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    let (size, fingerprint) = fingerprint_file(entry.path(), default_digest())?;
                    provenance.record_collision(
                        entry.path().display(),
                        fingerprint,
                        size,
                        dst_path,
                    );
                }
                Err(err) => bail!("{err}"),
            }
        }
        Ok(())
    })
}

fn populate_from_packed_pex<'a>(
    venv: &Virtualenv,
    packed_pex: &'a Pex<'a>,
    resolved_wheels: &IndexMap<&'a str, ResolvedWheel<'a>>,
    populate_pex_info: bool,
    scope: InstallScope,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let site_packages_path = venv.site_packages_path();
    if matches!(scope, InstallScope::All | InstallScope::Deps) {
        collect_wheels_from_directory_pex(packed_pex, resolved_wheels)?
            .into_par_iter()
            .try_for_each(|wheel_zip| {
                populate_whl_zip(&wheel_zip, &site_packages_path, provenance.clone())
            })?;
    }
    if matches!(scope, InstallScope::All | InstallScope::Srcs) {
        populate_user_code_from_directory_pex(
            packed_pex,
            venv,
            &site_packages_path,
            populate_pex_info,
            provenance,
        )?;
    }
    Ok(())
}

fn populate_whl_zip(
    wheel: &Path,
    site_packages_path: &Path,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let whl_zip = ZipArchive::new(File::open(wheel)?.into_file())?;
    let metadata = whl_zip.metadata();
    (0..whl_zip.len()).into_par_iter().try_for_each(|index| {
        let zip_fp = File::open(wheel)?;
        let mut zip = unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
        extract_idx(
            site_packages_path,
            index,
            &mut zip,
            wheel.display(),
            provenance.clone(),
        )
    })
}

fn populate_user_code_from_directory_pex<'a>(
    directory_pex: &'a Pex<'a>,
    venv: &Virtualenv,
    site_packages_path: &Path,
    populate_pex_info: bool,
    provenance: Arc<Provenance>,
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
            fs::create_dir_all(dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            match File::create_new(&dst_path) {
                Ok(mut dst) => {
                    provenance.record(entry.path().display(), dst_path);
                    io::copy(&mut File::open(entry.path())?, &mut dst)?;
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    let (size, fingerprint) = fingerprint_file(entry.path(), default_digest())?;
                    provenance.record_collision(
                        entry.path().display(),
                        fingerprint,
                        size,
                        dst_path,
                    );
                }
                Err(err) => bail!("{err}"),
            }
        }
        Ok(())
    })?;
    if populate_pex_info {
        fs::copy(
            directory_pex.path.join("PEX-INFO"),
            venv.prefix().join("PEX-INFO"),
        )?;
    }
    Ok(())
}

fn populate_from_zip_app_with_whl_deps<'a>(
    venv: &Virtualenv,
    zip_app_pex: &'a Pex<'a>,
    resolved_wheels: &IndexMap<&'a str, ResolvedWheel<'a>>,
    populate_pex_info: bool,
    scope: InstallScope,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let pex_zip = ZipArchive::new(File::open(zip_app_pex.path)?)?;
    let metadata = pex_zip.metadata();
    let site_packages_path = venv.site_packages_path();

    if matches!(scope, InstallScope::All | InstallScope::Deps) {
        let wheel_file_names = resolved_wheels.into_iter().collect::<Vec<_>>();
        wheel_file_names
            .into_par_iter()
            .try_for_each(|(wheel_file_name, _)| {
                let zip_fp = File::open(zip_app_pex.path)?;
                let mut zip =
                    unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
                let whl_file = zip.by_name_seek(&[".deps", wheel_file_name].join("/"))?;
                let whl_zip = ZipArchive::new(whl_file)?;
                let whl_zip_metadata = whl_zip.metadata();
                (0..whl_zip.len()).into_par_iter().try_for_each(|index| {
                    let zip_fp = File::open(zip_app_pex.path)?;
                    let mut zip =
                        unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
                    let whl_name = [".deps", wheel_file_name].join("/");
                    let whl_file = zip.by_name_seek(&whl_name)?;
                    let mut whl_zip = unsafe {
                        ZipArchive::unsafe_new_with_metadata(whl_file, whl_zip_metadata.clone())
                    };
                    extract_idx(
                        &site_packages_path,
                        index,
                        &mut whl_zip,
                        format!("{zip}/{whl_name}", zip = zip_app_pex.path.display()),
                        provenance.clone(),
                    )
                })
            })?;
    }
    if matches!(scope, InstallScope::All | InstallScope::Srcs) {
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
        extract_indexes
            .into_par_iter()
            .try_for_each(|index| -> anyhow::Result<()> {
                let zip_fp = File::open(zip_app_pex.path)?;
                let mut zip =
                    unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
                extract_idx(
                    &site_packages_path,
                    index,
                    &mut zip,
                    zip_app_pex.path.display(),
                    provenance.clone(),
                )?;
                Ok(())
            })?;
        if populate_pex_info {
            let mut pex_zip = ZipArchive::new(File::open(zip_app_pex.path)?)?;
            let mut pex_info_src_fp = pex_zip.by_name("PEX-INFO")?;
            let mut pex_info_dst_fp = File::create_new(venv.prefix().join("PEX-INFO"))?;
            io::copy(&mut pex_info_src_fp, &mut pex_info_dst_fp)?;
        }
    }
    Ok(())
}

struct DepFilter<'a>(&'a IndexMap<&'a str, ResolvedWheel<'a>>);

impl<'a> DepFilter<'a> {
    fn filter_deps(&self, file_name: &str) -> bool {
        file_name.starts_with(".deps/")
            && file_name[6..]
                .split("/")
                .next()
                .map(|whl_name| self.0.contains_key(whl_name))
                .unwrap_or_default()
    }
}

fn filter_srcs(file_name: &str) -> bool {
    ![".deps/", "__pex__/"]
        .iter()
        .any(|dir_prefix| file_name.starts_with(dir_prefix))
        && ![".deps/", "__pex__/", "PEX-INFO", "__main__.py"].contains(&file_name)
}

fn populate_from_zip_app<'a>(
    venv: &Virtualenv,
    zip_app_pex: &'a Pex<'a>,
    resolved_wheels: &IndexMap<&'a str, ResolvedWheel<'a>>,
    populate_pex_info: bool,
    scope: InstallScope,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let mut pex_zip = ZipArchive::new(File::open(zip_app_pex.path)?)?;
    let metadata = pex_zip.metadata();
    let dep_filter = DepFilter(resolved_wheels);
    let extract_indexes = pex_zip
        .file_names()
        .enumerate()
        .filter_map(|(idx, name)| {
            // TODO: XXX: Deal with .layout and .prefix/ in wheel chroots.
            if match scope {
                InstallScope::All => dep_filter.filter_deps(name) || filter_srcs(name),
                InstallScope::Deps => dep_filter.filter_deps(name),
                InstallScope::Srcs => filter_srcs(name),
            } {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let site_packages_path = venv.site_packages_path();
    extract_indexes
        .into_par_iter()
        .try_for_each(|index| -> anyhow::Result<()> {
            let zip_fp = File::open(zip_app_pex.path)?;
            let mut zip = unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
            extract_idx(
                &site_packages_path,
                index,
                &mut zip,
                zip_app_pex.path.display(),
                provenance.clone(),
            )?;
            Ok(())
        })?;
    if populate_pex_info && matches!(scope, InstallScope::All | InstallScope::Srcs) {
        let mut pex_info_src_fp = pex_zip.by_name("PEX-INFO")?;
        let mut pex_info_dst_fp = File::create_new(venv.prefix().join("PEX-INFO"))?;
        io::copy(&mut pex_info_src_fp, &mut pex_info_dst_fp)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[time("debug", "{}")]
pub fn populate_user_code_and_wheels<'a>(
    venv: &Virtualenv,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    pex: &'a Pex<'a>,
    resolved_wheels: IndexMap<&'a str, ResolvedWheel<'a>>,
    populate_pex_info: bool,
    scope: InstallScope,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    match pex.layout {
        Layout::Loose => populate_from_loose_pex(
            venv,
            pex,
            &resolved_wheels,
            populate_pex_info,
            scope,
            provenance.clone(),
        )?,
        Layout::Packed => populate_from_packed_pex(
            venv,
            pex,
            &resolved_wheels,
            populate_pex_info,
            scope,
            provenance.clone(),
        )?,
        Layout::ZipApp => {
            if pex.info.deps_are_wheel_files {
                populate_from_zip_app_with_whl_deps(
                    venv,
                    pex,
                    &resolved_wheels,
                    populate_pex_info,
                    scope,
                    provenance.clone(),
                )?
            } else {
                populate_from_zip_app(
                    venv,
                    pex,
                    &resolved_wheels,
                    populate_pex_info,
                    scope,
                    provenance.clone(),
                )?
            }
        }
    }
    if matches!(scope, InstallScope::All | InstallScope::Deps) {
        resolved_wheels
            .into_values()
            .collect::<Vec<_>>()
            .into_par_iter()
            .try_for_each(|resolved_wheel| {
                spread(
                    pex,
                    resolved_wheel,
                    venv,
                    shebang_interpreter,
                    shebang_arg,
                    provenance.clone(),
                )
            })?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[time("debug", "{}")]
pub fn populate<'a>(
    venv: &Virtualenv,
    shebang_interpreter: &Path,
    shebang_arg: Option<&str>,
    pex: &'a Pex<'a>,
    resolved_wheels: IndexMap<&'a str, ResolvedWheel<'a>>,
    scripts: &mut Scripts,
    bin_path_override: Option<BinPath>,
    scope: InstallScope,
    provenance: Arc<Provenance>,
) -> anyhow::Result<()> {
    let selected_wheels = resolved_wheels.keys().copied().collect::<Vec<_>>();
    populate_user_code_and_wheels(
        venv,
        shebang_interpreter,
        shebang_arg,
        pex,
        resolved_wheels,
        true,
        scope,
        provenance,
    )?;
    if matches!(scope, InstallScope::All | InstallScope::Srcs) {
        write_main(
            venv,
            shebang_interpreter,
            shebang_arg,
            &pex.info,
            scripts,
            bin_path_override,
        )?;
        write_repl(
            venv,
            shebang_interpreter,
            shebang_arg,
            pex.path,
            &pex.info,
            selected_wheels,
            scripts,
        )?;
    }
    Ok(())
}

fn extract_idx<R>(
    dst_dir: impl AsRef<Path>,
    index: usize,
    zip: &mut ZipArchive<R>,
    source: impl Display,
    provenance: Arc<Provenance>,
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
                provenance.record(format!("{source}/{name}", name = zip_file.name()), dst_path);
                io::copy(&mut zip_file, &mut dst_file)?;
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let size = usize::try_from(zip_file.size())?;
                let name = zip_file.name().to_string();
                let fingerprint = Fingerprint::try_from(BufReader::new(zip_file))?;
                provenance.record_collision(
                    format!("{source}/{name}"),
                    fingerprint,
                    size,
                    dst_path,
                );
            }
            Err(err) => bail!("{err}"),
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
    bin_path_override: Option<BinPath>,
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
            bin_path = bin_path_override
                .as_ref()
                .unwrap_or_else(|| pex_info.venv_bin_path.as_ref().unwrap_or(&BinPath::False))
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
