// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use std::env;
use std::str::FromStr;

use anyhow::anyhow;
use env_logger::Target;
use log::LevelFilter;

const DEFAULT_LEVEL: LevelFilter = LevelFilter::Warn;

pub fn init_default() -> anyhow::Result<()> {
    init(None)
}

pub fn init(level: Option<LevelFilter>) -> anyhow::Result<()> {
    let level_filter = if let Some(level_filter) = level {
        level_filter
    } else {
        calculate_level()?
    };
    env_logger::Builder::new()
        .target(Target::Stderr)
        .filter(None, level_filter)
        .init();
    Ok(())
}

fn calculate_level() -> anyhow::Result<LevelFilter> {
    parse_pex_level().map(|pex_filter| {
        pex_filter
            .iter()
            .copied()
            .chain(parse_rust_level().iter().copied())
            .max()
            .unwrap_or(DEFAULT_LEVEL)
    })
}

fn parse_pex_level() -> anyhow::Result<Option<LevelFilter>> {
    let level = if let Some(level_var) = env::var_os("PEX_VERBOSE") {
        let level_str = level_var.into_string().map_err(|raw_value| {
            anyhow!(
                "PEX_VERBOSE must be an un-signed integer. Given non-UTF-8 value: {value}",
                value = raw_value.display()
            )
        })?;
        match u8::from_str(&level_str).map_err(|err| {
            anyhow!("PEX_VERBOSE must be an un-signed integer. Given {level_str}: {err}")
        })? {
            0 => DEFAULT_LEVEL,
            1 => DEFAULT_LEVEL.increment_severity(),
            2 => DEFAULT_LEVEL.increment_severity().increment_severity(),
            _ => DEFAULT_LEVEL
                .increment_severity()
                .increment_severity()
                .increment_severity(),
        }
    } else {
        return Ok(None);
    };
    Ok(Some(level))
}

fn parse_rust_level() -> Option<LevelFilter> {
    if let Ok(level_var) = env::var("RUST_LOG")
        && let Ok(filter_builder) = env_filter::Builder::new().try_parse(&level_var)
    {
        Some(filter_builder.build().filter())
    } else {
        None
    }
}
