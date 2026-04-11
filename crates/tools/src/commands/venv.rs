// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cache::CacheDir;
use clap::builder::PossibleValue;
use clap::{Args, ValueEnum};
use fs_err as fs;
use interpreter::SearchPath;
use log::warn;
use pex::{Layout, Pex, PexPath};
use venv::virtualenv::FileSystemLinker;
use venv::{Virtualenv, venv_pex};

#[derive(Clone)]
struct BinPath(pex::BinPath);

impl BinPath {
    fn into_inner(self) -> pex::BinPath {
        self.0
    }
}

impl ValueEnum for BinPath {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            BinPath(pex::BinPath::False),
            BinPath(pex::BinPath::Append),
            BinPath(pex::BinPath::Prepend),
        ]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.0.as_str()))
    }
}

#[derive(Clone, Eq, PartialEq, ValueEnum)]
enum InstallScope {
    All,
    Deps,
    Srcs,
}

impl InstallScope {
    fn as_str(&self) -> &str {
        match self {
            InstallScope::All => "all",
            InstallScope::Deps => "deps",
            InstallScope::Srcs => "srcs",
        }
    }
}

#[derive(Clone, Eq, PartialEq, ValueEnum)]
enum RemoveScope {
    Pex,
    All,
}

#[derive(Args)]
pub(crate) struct VenvArgs {
    // *  --collisions-ok       Don't error if population of the ven-v encounters distributions in the PEX file with colliding files, just emit a warning. (default: False)
    //   -p, --pip             Add pip (and setuptools) to the venv. If the PEX already contains its own conflicting versions pip (or setuptools), the command will error and you must pass
    //                         --collisions-ok to have the PEX versions over-ride the natural venv versions installed by --pip. (default: False)
    // *  --copies              Create the venv using copies of system files instead of symlinks. (default: False)
    // *  --site-packages-copies
    //                         Create the venv using copies of distributions instead of links or symlinks. (default: False)
    //   --prompt PROMPT       A custom prompt for the venv activation scripts to use. (default: None)
    /// The scope of code contained in the Pex that is installed in the venv.
    #[arg(long, value_enum, default_value_t = InstallScope::All, long_help = "\
The scope of code contained in the Pex that is installed in the venv.
By default, all code is installed and this is generally what you want. However, in some situations
it's beneficial to split the venv installation into deps and srcs steps. This is particularly useful
when installing a PEX in a container image. See
https://docs.pex-tool.org/recipes.html#pex-app-in-a-container for more information.
"
    )]
    scope: InstallScope,

    /// Remove the PEX after creating a venv from it if the 'pex' value is specified; otherwise,
    /// remove the PEX and the PEX_ROOT if the 'all' value is specified.
    #[arg(long, value_enum)]
    remove: Option<RemoveScope>,

    /// Add the venv bin dir to the PATH in the __main__.py script.
    #[arg(short = 'b', long, value_enum)]
    bin_path: Option<BinPath>,

    /// Give the venv access to the system site-packages dir.
    #[arg(long, default_value_t = false)]
    system_site_packages: bool,

    /// Don't rewrite Python script shebangs in the venv to pass `-I` (or `-sE`).
    #[arg(
        long,
        default_value_t = false,
        long_help = "\
Don't rewrite Python script shebangs in the venv to pass `-I` (or `-sE`) to the interpreter.
This can be used to enable running the venv PEX itself or its Python scripts with a custom
`PYTHONPATH`.
"
    )]
    non_hermetic_scripts: bool,

    /// Compile all `.py` files in the venv.
    #[arg(long, default_value_t = false)]
    compile: bool,

    /// If the venv directory already exists, overwrite it.
    #[arg(short = 'f', long, default_value_t = false)]
    force: bool,

    /// The directory to create the venv in.
    #[arg()]
    venv_dir: PathBuf,
}

pub(crate) fn create(python: &Path, pex: Pex, args: VenvArgs) -> anyhow::Result<()> {
    if args.scope != InstallScope::All {
        todo!(
            "Support for --scope {scope} is under development.",
            scope = args.scope.as_str()
        );
    }

    let search_path = SearchPath::from_env()?;
    let pex_path = PexPath::from_pex_info(&pex.info, true);
    let additional_pexes = pex_path.load_pexes()?;
    let resolve = pex.resolve(Some(python), additional_pexes.iter(), search_path)?;
    let mut scripts = pex.scripts()?;

    if args.force && args.venv_dir.exists() {
        fs::remove_dir_all(&args.venv_dir)?;
    }
    fs::create_dir_all(&args.venv_dir)?;
    let venv = Virtualenv::create(
        resolve.interpreter,
        Cow::Borrowed(&args.venv_dir),
        FileSystemLinker(),
        &mut scripts,
        args.system_site_packages,
    )?;

    let shebang_arg = if args.non_hermetic_scripts {
        None
    } else if (
        venv.interpreter.version.major,
        venv.interpreter.version.minor,
    ) >= (3, 4)
    {
        Some("-I")
    } else {
        Some("-sE")
    };

    venv_pex::populate(
        &venv,
        &venv.interpreter.path,
        shebang_arg,
        &pex,
        resolve.wheels,
        &mut scripts,
        args.bin_path.map(BinPath::into_inner),
    )?;
    for (pex, wheels) in resolve.additional_wheels {
        venv_pex::populate_user_code_and_wheels(
            &venv,
            &venv.interpreter.path,
            shebang_arg,
            pex,
            wheels,
            false,
        )?;
    }

    if args.compile {
        let exit_status = Command::new(venv.interpreter.path)
            .args(["-m", "compileall"])
            .arg(args.venv_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .wait()?;
        if !exit_status.success() {
            warn!("Ignoring compile error: {exit_status}")
        }
    }

    if let Some(scope) = args.remove {
        match pex.layout {
            Layout::Loose | Layout::Packed => fs::remove_dir_all(pex.path)?,
            Layout::ZipApp => fs::remove_file(pex.path)?,
        }
        if scope == RemoveScope::All {
            let pex_root = if let Some(root) = pex.info.pex_root.as_deref() {
                Path::new(root)
            } else {
                CacheDir::root()?
            };
            if pex_root.exists() {
                fs::remove_dir_all(pex_root)?;
            }
        }
    }
    Ok(())
}
