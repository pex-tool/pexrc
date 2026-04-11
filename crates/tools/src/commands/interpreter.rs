// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

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

pub(crate) fn display(
    python: &Path,
    pex: Pex,
    all: bool,
    verbosity: u8,
    indent: Option<u8>,
    output: Option<&Path>,
) -> anyhow::Result<()> {
    for interpreter in compatible_interpreters(python, &pex, all)? {
        match verbosity {
            0 => {
                if let Some(indent) = indent {
                    warn!("Ignoring --indent={indent} since --verbose mode is not enabled.")
                }
                crate::output::using(output, |mut out| {
                    writeln!(out, "{path}", path = interpreter.path.display())
                })?
            }
            1 => crate::json::serialize(
                &json!({
                    "path": interpreter.path,
                    "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                    "platform": Platform::of(&interpreter)?.to_string()
                }),
                indent,
                output,
            )?,
            2 => crate::json::serialize(
                &json!({
                    "path": interpreter.path,
                    "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                    "platform": Platform::of(&interpreter)?.to_string(),
                    "supported_tags": interpreter.supported_tags
                }),
                indent,
                output,
            )?,
            _ => {
                if interpreter.is_venv() {
                    let mut scripts = pex.scripts()?;
                    let base_interpreter =
                        interpreter.clone().resolve_base_interpreter(&mut scripts)?;
                    crate::json::serialize(
                        &json!({
                            "path": interpreter.path,
                            "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                            "platform": Platform::of(&interpreter)?.to_string(),
                            "supported_tags": interpreter.supported_tags,
                            "env_markers": interpreter.marker_env,
                            "venv": true,
                            "base_interpreter": base_interpreter.path
                        }),
                        indent,
                        output,
                    )?
                } else {
                    crate::json::serialize(
                        &json!({
                            "path": interpreter.path,
                            "requirement": InterpreterConstraint::exact_version(&interpreter).to_string(),
                            "platform": Platform::of(&interpreter)?.to_string(),
                            "supported_tags": interpreter.supported_tags,
                            "env_markers": interpreter.marker_env,
                            "venv": false
                        }),
                        indent,
                        output,
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
            pex.resolve(Some(python), [].iter(), search_path.clone())?
                .interpreter
        ];
        let mut scripts = pex.scripts()?;
        let identification_script = IdentifyInterpreter::read(&mut scripts)?;
        let interpreter_constraints =
            InterpreterConstraints::try_from(&pex.info.interpreter_constraints)?;
        let resolved = pex.resolve_all(
            &identification_script,
            &interpreter_constraints,
            search_path,
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
            pex.resolve(Some(python), [].iter(), search_path)?
                .interpreter
        ])
    }
}
