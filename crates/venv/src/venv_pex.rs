// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::{fs, io};

use anyhow::anyhow;
use indexmap::IndexSet;
use itertools::Itertools;
use log::warn;
use logging_timer::time;
use pex::{BinPath, Pex, PexInfo};
use platform::{link_or_copy, mark_executable, path_as_bytes, path_as_str};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use zip::ZipArchive;

use crate::virtualenv::Virtualenv;

const VENV_PEX_PY: &[u8] = include_bytes!("venv_pex.py");
const VENV_PEX_REPL_PY: &[u8] = include_bytes!("venv_pex_repl.py");

#[time("debug", "venv_pex.{}")]
pub fn populate(
    venv: &Virtualenv,
    resting_venv_dir: &Path,
    pex: &Pex,
    selected_wheels: &IndexSet<&str>,
) -> anyhow::Result<()> {
    let site_packages_path = venv.site_packages_path();
    let (path, pex_info) = match pex {
        Pex::Loose(_) => todo!("XXX: Implement loose PEX venv population."),
        Pex::Packed(_) => todo!("XXX: Implement packed PEX venv population."),
        Pex::ZipApp(zip_app_pex) => {
            let mut pex_zip = ZipArchive::new(File::open(zip_app_pex.0)?)?;
            let metadata = pex_zip.metadata();
            let extract_indexes = pex_zip
                .file_names()
                .enumerate()
                .filter_map(|(idx, name)| {
                    // TODO: XXX: Deal with .layout and .prefix/ in wheel chroots.
                    if [".bootstrap/", "__pex__/"]
                        .iter()
                        .any(|exclude_dir| name.starts_with(exclude_dir))
                        || ["PEX-INFO", "__main__.py", ".deps/"].contains(&name)
                        || name.starts_with(".deps/")
                            && name[6..]
                                .split("/")
                                .next()
                                .map(|whl_name| !selected_wheels.contains(whl_name))
                                .unwrap_or(true)
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
                    let zip_fp = File::open(zip_app_pex.0)?;
                    let mut zip =
                        unsafe { ZipArchive::unsafe_new_with_metadata(zip_fp, metadata.clone()) };
                    extract_idx(&site_packages_path, index, &mut zip)?;
                    Ok(())
                })?;
            let mut pex_info_src_fp = pex_zip.by_name("PEX-INFO")?;
            let mut pex_info_dst_fp = File::create_new(venv.prefix().join("PEX-INFO"))?;
            io::copy(&mut pex_info_src_fp, &mut pex_info_dst_fp)?;
            (zip_app_pex.0, &zip_app_pex.1)
        }
    };

    write_main(venv, resting_venv_dir, pex_info)?;
    write_repl(venv, resting_venv_dir, path, pex_info)
}

fn extract_idx(
    dst_dir: impl AsRef<Path>,
    index: usize,
    zip: &mut ZipArchive<File>,
) -> anyhow::Result<()> {
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
    venv: &Virtualenv,
    resting_venv_dir: &Path,
) -> anyhow::Result<()> {
    let interpreter_relpath = venv
        .interpreter
        .path
        .strip_prefix(&venv.interpreter.prefix)?;
    for chunk in [
        b"#!",
        path_as_bytes(&resting_venv_dir.join(interpreter_relpath))?,
        b"\n",
    ] {
        file.write_all(chunk)?;
    }
    Ok(())
}

fn as_python_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

fn as_optional_python_str(value: Option<&str>) -> Cow<'_, str> {
    if let Some(value) = value {
        Cow::Owned(format!("r\"{value}\""))
    } else {
        Cow::Borrowed("None")
    }
}

fn write_main(
    venv: &Virtualenv,
    resting_venv_dir: &Path,
    pex_info: &PexInfo,
) -> anyhow::Result<()> {
    let mut main_py_fp = File::create_new(venv.prefix().join("__main__.py"))?;

    write_shebang_bytes(&mut main_py_fp, venv, resting_venv_dir)?;
    main_py_fp.write_all(VENV_PEX_PY)?;
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
        inject_env={{{inject_env}}},
        inject_args=[{inject_args}],
        entry_point={entry_point},
        script={script},
        hermetic_re_exec={hermetic_re_exec},
    )
"#,
            shebang_python = path_as_str(&venv.interpreter.path)?,
            venv_bin_dir = venv.bin_dir_relpath,
            bin_path = pex_info
                .venv_bin_path
                .as_ref()
                .unwrap_or(&BinPath::False)
                .as_str(),
            strip_pex_env = as_python_bool(pex_info.strip_pex_env.unwrap_or(true)),
            inject_env = pex_info
                .inject_env
                .iter()
                .map(|(k, v)| format!("r\"{k}\":r\"{v}\""))
                .join(","),
            inject_args = pex_info
                .inject_args
                .iter()
                .map(|v| format!("r\"{v}\""))
                .join(","),
            entry_point = as_optional_python_str(pex_info.entry_point.as_deref()),
            script = as_optional_python_str(pex_info.script.as_deref()),
            hermetic_re_exec = as_optional_python_str(if pex_info.venv_hermetic_scripts {
                Some(venv.interpreter.hermetic_args())
            } else {
                None
            })
        )
    )?;
    mark_executable(&mut main_py_fp)?;
    link_or_copy(Path::new("__main__.py"), venv.prefix().join("pex"))
}

fn write_repl(
    venv: &Virtualenv,
    resting_venv_dir: &Path,
    pex: &Path,
    pex_info: &PexInfo,
) -> anyhow::Result<()> {
    let mut pex_repl_py_fp = File::create_new(venv.prefix().join("pex-repl"))?;
    write_shebang_bytes(&mut pex_repl_py_fp, venv, resting_venv_dir)?;
    pex_repl_py_fp.write_all(VENV_PEX_REPL_PY)?;
    // TODO: XXX: Need to append a if __name__ == "__main__" that calls _create_pex_repl(...)
    // const activation_summary, const activation_details = res: {
    //         if (wheels_to_install.*) |wheels| {
    //             const summary = try std.fmt.allocPrint(
    //                 allocator,
    //                 "{d} {s} and {d} activated {s}",
    //                 .{
    //                     self.pex_info.requirements.len,
    //                     if (self.pex_info.requirements.len > 1) "requirements" else "requirement",
    //                     wheels.entries.len,
    //                     if (wheels.entries.len > 1) "distributions" else "distribution",
    //                 },
    //             );
    //             errdefer allocator.free(summary);
    //
    //             var details = std.ArrayList(u8).init(allocator);
    //             errdefer details.deinit();
    //
    //             var details_writer = details.writer();
    //             try details_writer.writeAll("Requirements:\n");
    //             for (self.pex_info.requirements) |requirement| {
    //                 try details_writer.writeAll("  ");
    //                 try details_writer.writeAll(requirement);
    //                 try details_writer.writeByte('\n');
    //             }
    //             try details_writer.writeAll("Activated Distributions:\n");
    //             for (wheels.entries) |wheel| {
    //                 try details_writer.writeAll("  ");
    //                 try details_writer.writeAll(wheel.name);
    //                 try details_writer.writeByte('\n');
    //             }
    //             break :res .{ summary, try details.toOwnedSlice() };
    //         } else {
    //             break :res .{ "no dependencies", "" };
    //         }
    //     };
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
            activation_summary = "",
            activation_details = "",
        )
    )?;
    mark_executable(&mut pex_repl_py_fp)?;

    Ok(())
}
