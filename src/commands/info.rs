// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::cmp;

use owo_colors::OwoColorize;

use crate::clibs::CLIBS_DIR;

pub fn display() {
    let mut paths = Vec::new();
    let mut max_width = 0;
    for clib in CLIBS_DIR.files() {
        let path = clib.path().display().to_string();
        max_width = cmp::max(max_width, path.len());
        paths.push(path);
    }
    let count = paths.len();
    anstream::println!(
        "There are {count} embedded {clibs}:",
        count = count.yellow(),
        clibs = if count == 1 { "clib" } else { "clibs" }
    );
    for (idx, (clib, path)) in CLIBS_DIR.files().zip(paths).enumerate() {
        anstream::println!(
            "{idx:>3}. {path} {pad}{size:<7} bytes",
            idx = (idx + 1).yellow(),
            path = path.blue(),
            pad = " ".repeat(max_width - path.len()),
            size = clib.contents().len().yellow()
        )
    }
}
