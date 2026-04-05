// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, mem};

use anyhow::bail;
use cache::{CacheDir, HashOptions, Key, atomic_dir};
use interpreter::{Interpreter, SearchPath};
use itertools::Itertools;
use log::{info, warn};
use logging_timer::time;
use pex::{Pex, PexPath};
use regex::bytes::Regex;
use venv::{Linker, Virtualenv, populate, populate_user_code_and_wheels};

struct PythonProxyLinker<'a>(&'a Pex<'a>);

impl<'a> Linker for PythonProxyLinker<'a> {
    fn link(&self, interpreter: &Interpreter, path: &Path) -> anyhow::Result<()> {
        python_proxy::create(self.0, interpreter, path, None)
    }
}

pub fn boot(
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path>,
    argv: Vec<String>,
) -> anyhow::Result<i32> {
    let lock = match cache::read_lock() {
        Ok(lock) => lock,
        Err(err) => bail!("Failed to obtain PEXRC cache read lock: {err}"),
    };
    let mut command = prepare_boot(python, python_args, pex, argv)?;
    info!(
        "Booting with {exe} {args}",
        exe = command.get_program().to_string_lossy(),
        args = command.get_args().map(OsStr::to_string_lossy).join(" ")
    );
    Ok(platform::exec(&mut command, &[lock])?)
}

fn prepare_boot(
    python: impl AsRef<Path>,
    python_args: Vec<String>,
    pex: impl AsRef<Path>,
    argv: Vec<String>,
) -> anyhow::Result<Command> {
    logging::init_default()?;
    let venv = prepare_venv(
        python,
        pex.as_ref(),
        #[cfg(unix)]
        env::var_os("_PEXRC_SH_BOOT_SEED_DIR").map(PathBuf::from),
    )?;
    let mut command = Command::new(&venv.interpreter.path);
    command
        .args(python_args)
        .arg(venv.prefix().as_os_str())
        .args(argv);
    Ok(command)
}

pub fn mount(python: impl AsRef<Path>, pex: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    match cache::read_lock() {
        Ok(lock) => {
            // N.B.: We're being called from a Python program that lives longer than us via an
            // import hook. We want the cache read lock to survive for the lifetime of that process.
            // To prevent unlock when the lock goes out of scope, we forget it to disable its
            // destructor. This will keep the fd open which will keep the read_lock read-locked.
            mem::forget(lock);
        }
        Err(err) => bail!("Failed to obtain PEXRC cache read lock: {err}"),
    };

    logging::init_default()?;
    prepare_venv(
        python,
        pex.as_ref(),
        #[cfg(unix)]
        None,
    )
    .map(|venv| venv.site_packages_path())
}

#[time("debug", "{}")]
fn prepare_venv<'a>(
    python: impl AsRef<Path>,
    pex: impl AsRef<Path>,
    #[cfg(unix)] sh_boot_seed_dir: Option<PathBuf>,
) -> anyhow::Result<Virtualenv<'a>> {
    let pex = Pex::load(pex.as_ref())?;
    let pex_path = PexPath::from_pex_info(&pex.info, true);
    let additional_pexes = pex_path.load_pexes()?;
    let search_path = SearchPath::from_env()?;
    let venv_dir = venv_dir(Some(python.as_ref()), &pex, &search_path, &additional_pexes)?;
    if let Some(venv_interpreter) = atomic_dir(&venv_dir, |work_dir| {
        let mut resolve =
            pex.resolve(Some(python.as_ref()), additional_pexes.iter(), search_path)?;
        let venv = Virtualenv::create(
            resolve.interpreter,
            Cow::Borrowed(work_dir),
            PythonProxyLinker(&pex),
            &mut resolve.scripts,
            pex.info.venv_system_site_packages,
        )?;
        populate(&venv, &venv_dir, &pex, resolve.wheels, &mut resolve.scripts)?;
        for (additional_pex, resolved_wheels) in resolve.additional_wheels {
            populate_user_code_and_wheels(&venv, additional_pex, resolved_wheels, false)?;
        }
        Ok(venv.interpreter)
    })? {
        info!("Built venv at {path}", path = venv_dir.display());
        let venv_interpreter = Virtualenv::host_interpreter(&venv_dir, &venv_interpreter);
        venv_interpreter.store()?;
        #[cfg(unix)]
        if let Some(sh_boot_seed_dir) = sh_boot_seed_dir {
            fs_err::create_dir_all(&sh_boot_seed_dir)?;
            platform::unix::symlink(
                venv_dir.join("pex"),
                sh_boot_seed_dir.join(venv_interpreter.most_specific_exe_name()),
                true,
            )?;
        }
        Virtualenv::enclosing(venv_interpreter)
    } else {
        info!("Loading cached venv at {path}", path = venv_dir.display());
        let mut scripts = pex.scripts()?;
        Virtualenv::load(Cow::Owned(venv_dir), &mut scripts)
    }
}

const INTERPRETER_HASH_OPTIONS: HashOptions = HashOptions::new().path(true).mtime(true).size(true);

pub fn venv_dir(
    ambient_python: Option<&Path>,
    pex: &Pex,
    search_path: &SearchPath,
    additional_pexes: &[Pex],
) -> anyhow::Result<PathBuf> {
    let mut key = Key::default();

    // The primary PEX hash covers its user code contents, distributions and ICs.
    key.property("pex_hash", &pex.info.pex_hash);

    // We hash just the distributions of additional PEXes since those are the only items used from
    // PEX_PATH adjoined PEX files; i.e.: neither the entry_point nor any other PEX file data or
    // metadata is used.
    for additional_pex in additional_pexes {
        key.object("additional_pex", additional_pex.info.distributions.iter());
    }

    let mut imprecise_pex_python: Option<&OsStr> = None;
    let mut imprecise_pex_python_path: Option<OsString> = None;

    // If there are no restrictions on interpreter, whatever we derive from the ambient python is
    // our opaque choice, which we can keep.
    if pex.info.interpreter_constraints.is_empty()
        && search_path.is_empty()
        && let Some(python_exe) = ambient_python
    {
        key.file(python_exe, &INTERPRETER_HASH_OPTIONS)?;
    } else if let Some(python_exe) = search_path.unique_interpreter() {
        // The user chose a unique interpreter (via PEX_PYTHON or PEX_PYTHON_PATH or a combination
        // of the two). It may not match the ICs, if any, but the choice is respected.
        key.file(python_exe, &INTERPRETER_HASH_OPTIONS)?;
    } else {
        // Otherwise, we do our best.
        if let Some(python) = search_path.pex_python() {
            let value = python.as_encoded_bytes();
            key.property("PEX_PYTHON", value);
            if pex.info.emit_warnings
                && !Regex::new(r"^(?:[Pp]ython|pypy)\d+\.\d+[^\d]?(?:\.exe)$")?.is_match(value)
            {
                imprecise_pex_python = Some(python);
            }
        }
        if let Some(path) = search_path.pex_python_path() {
            key.list(
                "PEX_PYTHON_PATH",
                path.iter().map(|path| path.as_os_str().as_encoded_bytes()),
            );
            if pex.info.emit_warnings {
                imprecise_pex_python_path = Some(env::join_paths(path)?);
            }
        }
    }

    let venv_dir = CacheDir::Venv.path()?.join(PathBuf::from(key));
    if let Some(pex_python) = imprecise_pex_python {
        warn!(
            "\
            Using a venv selected by PEX_PYTHON={pex_python}\n\
            for {pex_file}\n\
            at {venv_dir}.\n\
            \n\
            If `{pex_python}` is upgraded or downgraded at some later date, this venv will still\n\
            be used. To force re-creation of the venv using the upgraded or downgraded\n\
            `{pex_python}` you will need to delete it at that point in time.\n\
            \n\
            To avoid this warning, either specify a Python binary with major and minor version\n\
            in its name, like PEX_PYTHON=python3.14 or else re-build the PEX\n\
            with `--no-emit-warnings` or re-run the PEX with PEX_EMIT_WARNINGS=False.\n\
            ",
            pex_python = pex_python.display(),
            pex_file = pex.path.display(),
            venv_dir = venv_dir.display()
        )
    }
    if let Some(pex_python_path) = imprecise_pex_python_path {
        warn!(
            "\
            Using a venv restricted by PEX_PYTHON_PATH={ppp}\n\
            for {pex_file}\n\
            at {venv_dir}.\n\
            \n\
            If the contents of `{ppp}` changes at some later date, this venv and the interpreter\n\
            selected from `{ppp}` will still be used. To force re-creation of the venv using\n\
            the new pythons available on `{ppp}` you will need to delete it at that point in\n\
            time.\n\
            \n\
            To avoid this warning, re-build the PEX with `--no-emit-warnings` or re-run the PEX\n\
            with PEX_EMIT_WARNINGS=False.\n\
            ",
            ppp = pex_python_path.display(),
            pex_file = pex.path.display(),
            venv_dir = venv_dir.display()
        )
    }
    Ok(venv_dir)
}
