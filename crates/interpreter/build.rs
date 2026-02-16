// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::env;

fn main() {
    if env::var("CARGO_FEATURE_BUILD_TESTS").is_ok() {
        // N.B.: We're passing these through to test-time for the build_target crate to see.
        for (name, value) in env::vars_os().filter_map(|(name, value)| {
            if let (Ok(name), Ok(value)) = (name.into_string(), value.into_string())
                && (name == "PROFILE" || name == "TARGET" || name.starts_with("CARGO_CFG_TARGET_"))
            {
                return Some((name, value));
            }
            None
        }) {
            println!("cargo:rustc-env={name}={value}");
        }
    }
}
