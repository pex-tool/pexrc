// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

#![deny(clippy::all)]

use build_target::Os;

fn main() {
    let scripts_dir = match build_target::target_os() {
        Os::Windows => "Scripts",
        _ => "bin",
    };
    println!("cargo::rustc-env=SCRIPTS_DIR={scripts_dir}");
}
