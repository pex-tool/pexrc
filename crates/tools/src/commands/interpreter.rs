// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;
use indexmap::indexset;
use interpreter::{
    Interpreter,
    InterpreterConstraint,
    InterpreterConstraints,
    Platform,
    SearchPath,
};
use log::warn;
use pex::{Pex, ResolvedWheels};
use rayon::iter::ParallelIterator;
use scripts::IdentifyInterpreter;
use serde_json::json;

use crate::output::Output;

#[derive(Args)]
pub(crate) struct InterpreterArgs {
    /// Print all compatible interpreters, preferred first.
    #[arg(short = 'a', long, default_value_t = false)]
    all: bool,

    /// Provide more information about the interpreter in JSON format.
    #[arg(short = 'v', long, action = clap::ArgAction::Count, long_help = "\
Provide more information about the interpreter in JSON format.
Once: include the interpreter requirement and platform in addition to its path.
Twice: include the interpreter's supported tags.
Thrice: include the interpreter's environment markers and its venv affiliation, if any."
    )]
    verbose: u8,

    /// Pretty-print verbose output JSON with the given indent.
    #[arg(short = 'i', long)]
    indent: Option<u8>,

    /// A file to output the Python interpreter path to; STDOUT by default.
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

pub(crate) fn display(python: &Path, pex: Pex, args: InterpreterArgs) -> anyhow::Result<()> {
    let mut out = Output::new(args.output.as_deref())?;
    for interpreter in compatible_interpreters(python, &pex, args.all)? {
        let raw_interpeter = interpreter.raw();
        match args.verbose {
            0 => {
                if let Some(indent) = args.indent {
                    warn!("Ignoring --indent={indent} since --verbose mode is not enabled.")
                }
                writeln!(&mut out, "{path}", path = raw_interpeter.path.display())?
            }
            1 => crate::json::serialize(
                &mut out,
                &json!({
                    "path": raw_interpeter.path,
                    "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                    "platform": Platform::of(&interpreter)?.to_string()
                }),
                args.indent,
            )?,
            2 => crate::json::serialize(
                &mut out,
                &json!({
                    "path": raw_interpeter.path,
                    "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                    "platform": Platform::of(&interpreter)?.to_string(),
                    "supported_tags": raw_interpeter.supported_tags
                }),
                args.indent,
            )?,
            _ => {
                if interpreter.is_venv() {
                    let mut scripts = pex.scripts()?;
                    let base_interpreter =
                        interpreter.clone().resolve_base_interpreter(&mut scripts)?;
                    crate::json::serialize(
                        &mut out,
                        &json!({
                            "path": raw_interpeter.path,
                            "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                            "platform": Platform::of(&interpreter)?.to_string(),
                            "supported_tags": raw_interpeter.supported_tags,
                            "env_markers": raw_interpeter.marker_env,
                            "venv": true,
                            "base_interpreter": base_interpreter.raw().path
                        }),
                        args.indent,
                    )?
                } else {
                    crate::json::serialize(
                        &mut out,
                        &json!({
                            "path": raw_interpeter.path,
                            "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                            "platform": Platform::of(&interpreter)?.to_string(),
                            "supported_tags": raw_interpeter.supported_tags,
                            "env_markers": raw_interpeter.marker_env,
                            "venv": false
                        }),
                        args.indent,
                    )?
                }
            }
        }
    }
    Ok(())
}

fn compatible_interpreters(
    python: &Path,
    pex: &Pex,
    all: bool,
) -> anyhow::Result<impl IntoIterator<Item = Interpreter>> {
    let search_path = SearchPath::from_env()?;
    if all {
        let mut interpreters = indexset![
            pex.resolve(Some(python), [].iter(), search_path.clone(), None)?
                .interpreter
        ];
        let mut scripts = pex.scripts()?;
        let identification_script = IdentifyInterpreter::read(&mut scripts)?;
        let interpreter_constraints =
            InterpreterConstraints::try_from(&pex.info.raw().interpreter_constraints)?;
        let resolved = pex.resolve_all(
            &identification_script,
            &interpreter_constraints,
            search_path,
            None,
        )?;
        let filter = |result| match result {
            Ok(ResolvedWheels { interpreter, .. }) => Some(interpreter),
            Err(_) => None,
        };
        for interpreter in resolved.filter_map(filter).collect::<Vec<_>>() {
            interpreters.insert(interpreter);
        }
        Ok(interpreters)
    } else {
        Ok(indexset![
            pex.resolve(Some(python), [].iter(), search_path, None)?
                .interpreter
        ])
    }
}
