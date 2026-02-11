// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use pexrs::boot as rust_boot;
use std::ffi::{CStr, c_char};

use std::path::Path;

#[inline]
unsafe fn as_path(value: *const c_char, subject: &str) -> Result<&Path, String> {
    // SAFETY: value must be a ull-terminated character array,
    let cstr = unsafe { CStr::from_ptr(value) };
    cstr.to_str()
        .map(Path::new)
        // TODO: Handle valid WTF-8 on Windows but not valid UTF-8?
        .map_err(|e| format!("{subject} path is not valid UTF-8: {e}"))
}

/// # Safety
///
/// The caller must ensure both `python_exe` and `pex_file` are null-terminated character arrays.
#[unsafe(export_name = "boot")]
pub unsafe extern "C" fn boot(python_exe: *const c_char, pex_file: *const c_char) -> u8 {
    // SAFETY: python_exe must be a null-terminated character array,
    let python_exe_path = match unsafe { as_path(python_exe, "Python executable") } {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };

    // SAFETY: pex_file must be a null-terminated character array,
    let pex_path = match unsafe { as_path(pex_file, "PEX file") } {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };

    match rust_boot(python_exe_path, pex_path, None, true) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!(
                "Problem booting PEX at {pex_path}: {err}",
                pex_path = pex_path.display()
            );
            1
        }
    }
}
