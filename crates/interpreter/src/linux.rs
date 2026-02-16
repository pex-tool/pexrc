// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::io::BufRead;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{anyhow, bail};
use elf::ElfStream;
use elf::endian::{AnyEndian, EndianParse};
use elf::file::{Class, FileHeader};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct LibcVersion {
    major: u8,
    minor: u8,
    patch: Option<u8>,
}

impl LibcVersion {
    fn parse(version: &str) -> anyhow::Result<Self> {
        let mut components_iter = version.split('.');
        let mut parse_component = |subject| -> anyhow::Result<u8> {
            let component = components_iter.next().ok_or_else(|| {
                anyhow!(
                    "Invalid musl libc version {version}: failed to parse {subject} version number"
                )
            })?;
            component.parse::<u8>().map_err(|err| {
                anyhow!("Failed to parse {subject} version component of {version}: {err}")
            })
        };
        let major = parse_component("major")?;
        let minor = parse_component("minor")?;
        let patch = parse_component("patch").ok();
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Manylinux {
    glibc: Option<LibcVersion>,
    armhf: bool,
    i686: bool,
}

impl Manylinux {
    fn from_header(
        header: FileHeader<AnyEndian>,
        glibc_version: Option<LibcVersion>,
    ) -> anyhow::Result<Self> {
        let _32_bit_little_endian =
            matches!(header.class, Class::ELF32) && header.endianness.is_little();
        let armhf = {
            if !_32_bit_little_endian || header.e_machine != elf::abi::EM_ARM {
                false
            } else {
                // The e_flags for 32-bit arm are documented here:
                // https://github.com/ARM-software/abi-aa/blob/main/aaelf32/aaelf32.rst#52elf-header
                const EF_ARM_ABIMASK: u32 = 0xFF000000;
                const EF_ARM_ABI_VER5: u32 = 0x05000000;
                const EF_ARM_ABI_FLOAT_HARD: u32 = 0x00000400;
                if header.e_flags & EF_ARM_ABIMASK != EF_ARM_ABI_VER5 {
                    false
                } else {
                    header.e_flags & EF_ARM_ABI_FLOAT_HARD == EF_ARM_ABI_FLOAT_HARD
                }
            }
        };
        let i686 = _32_bit_little_endian && header.e_machine == elf::abi::EM_386;
        Ok(Manylinux {
            glibc: glibc_version,
            armhf,
            i686,
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) enum LinuxInfo {
    #[serde(rename = "manylinux")]
    ManyLinux(Manylinux),
    #[serde(rename = "musllinux")]
    MuslLinux(LibcVersion),
}

impl LinuxInfo {
    pub(crate) fn parse(exe: impl AsRef<Path>) -> anyhow::Result<Self> {
        let exe_fp = File::open(&exe)?;
        let mut elf: ElfStream<AnyEndian, _> = ElfStream::open_stream(exe_fp)?;
        let segments = elf.segments().to_owned();
        for program_header in segments {
            if program_header.p_type != elf::abi::PT_INTERP {
                continue;
            }
            let (interpreter_path, is_musl) = {
                let interpreter = elf.segment_data(&program_header)?;
                let is_musl = interpreter
                    .windows(b"musl".len())
                    .any(|window| window == b"musl");
                let eos = interpreter
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(interpreter.len());
                (Path::new(OsStr::from_bytes(&interpreter[0..eos])), is_musl)
            };
            if is_musl {
                // N.B.: Support for the Version field is in musl >= 0.9.15 only (01/03/2014) but
                // musllinux support was only added in https://peps.python.org/pep-0656/ in 2021:
                // $ docker run --rm -it python:alpine /lib/ld-musl-x86_64.so.1 >/dev/null
                // musl libc (x86_64)
                // Version 1.2.5
                // Dynamic Program Loader
                // Usage: /lib/ld-musl-x86_64.so.1 [options] [--] pathname [args]
                let output = Command::new(interpreter_path)
                    .stderr(Stdio::piped())
                    .spawn()?
                    .wait_with_output()?;
                for line in output.stderr.lines() {
                    if let Ok(line) = line
                        && line.starts_with("Version ")
                    {
                        return Ok(LinuxInfo::MuslLinux(LibcVersion::parse(
                            line["Version ".len()..].trim(),
                        )?));
                    }
                }
                bail!(
                    "Failed to identify musl libc version for {interpreter}",
                    interpreter = interpreter_path.display()
                )
            } else {
                // N.B.: Support for --version in glibc >= 2.33 only (01/02/2021)
                // used by >= ubuntu:21.04. The manylinux spec started with
                // https://peps.python.org/pep-0513/ in 2016; so this does not cover
                // it.
                // $ /lib64/ld-linux-x86-64.so.2 --version 2>/dev/null
                // ld.so (Ubuntu GLIBC 2.41-6ubuntu1) stable release version 2.41.
                // Copyright (C) 2025 Free Software Foundation, Inc.
                // This is free software; see the source for copying conditions.
                // There is NO warranty; not even for MERCHANTABILITY or FITNESS FOR A
                // PARTICULAR PURPOSE.
                let mut glibc_version: Option<LibcVersion> = None;
                {
                    let result = Command::new(interpreter_path)
                        .arg("--version")
                        .stdout(Stdio::piped())
                        .spawn()
                        .and_then(Child::wait_with_output);
                    if let Ok(output) = result {
                        for line in output.stdout.lines() {
                            if let Ok(line) = line
                                && let Some(index) = line.find("release version ")
                            {
                                if let Ok(libc_version) = LibcVersion::parse(
                                    line[index + "release version ".len()..].trim(),
                                ) {
                                    glibc_version = Some(libc_version);
                                }
                                break;
                            }
                        }
                    }
                }
                return Ok(LinuxInfo::ManyLinux(Manylinux::from_header(
                    elf.ehdr,
                    glibc_version,
                )?));
            }
        }
        bail!(
            "Failed to gather information about the libc linked by {exe}",
            exe = exe.as_ref().display()
        );
    }
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod test {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use build_target::Env;

    use crate::linux::LinuxInfo;

    #[test]
    fn test_parse() {
        let assert_linux_info = if matches!(build_target::target_env(), Some(Env::Musl)) {
            |linux_info: LinuxInfo| assert!(matches!(linux_info, LinuxInfo::MuslLinux(_)))
        } else {
            |linux_info: LinuxInfo| {
                let manylinux = match linux_info {
                    LinuxInfo::ManyLinux(manylinux) => manylinux,
                    LinuxInfo::MuslLinux(libc_version) => {
                        panic!("Expected manylinux but detected musl {libc_version:?}")
                    }
                };
                assert_eq!(
                    matches!(build_target::target_env(), Some(Env::Gnu)),
                    manylinux.glibc.is_some(),
                    "The build target is {target:#?}",
                    target = build_target::target()
                );
                if manylinux.armhf {
                    assert_eq!(
                        build_target::PointerWidth::U32,
                        build_target::target_pointer_width()
                    );
                    assert_eq!(build_target::Endian::Little, build_target::target_endian());
                    assert_eq!(build_target::Arch::Arm, build_target::target_arch());
                }
                if manylinux.i686 {
                    assert_eq!(
                        build_target::PointerWidth::U32,
                        build_target::target_pointer_width()
                    );
                    assert_eq!(build_target::Endian::Little, build_target::target_endian());
                    assert_eq!(build_target::Arch::X86, build_target::target_arch());
                }
            }
        };
        let python_exe_bytes = Command::new("uv")
            .args(["python", "find"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap()
            .wait_with_output()
            .unwrap()
            .stdout;
        let python_exe = PathBuf::from(String::from_utf8(python_exe_bytes).unwrap().trim());
        let linux_info = LinuxInfo::parse(python_exe).unwrap();
        assert_linux_info(linux_info);
    }
}
