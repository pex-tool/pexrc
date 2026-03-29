# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

import collections
import functools
import json
import os
import platform
import re
import struct
import subprocess
import sys
import sysconfig
from argparse import ArgumentParser
from contextlib import contextmanager

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import (  # noqa: F401
        IO,
        Any,
        DefaultDict,
        Dict,
        Iterable,
        Iterator,
        List,
        Optional,
        Sequence,
        TextIO,
        Tuple,
        Type,
        Union,
        cast,
    )
else:

    def cast(
        _type,  # type: Union[str, Type]
        value,  # type: Any
    ):
        return value


def implementation_name_and_version():
    # type: () -> Tuple[str, str]
    implementation = getattr(sys, "implementation", None)
    if implementation:
        implementation_version_info = implementation.version
        version = "{0.major}.{0.minor}.{0.micro}".format(implementation_version_info)
        kind = implementation_version_info.releaselevel
        if kind != "final":
            version += kind[0] + str(implementation_version_info.serial)
        return implementation.name, version
    return "", "0"


def identify(supported_tags):
    # type: (Iterable[Tuple[str, str, str]]) -> Dict[str, Any]

    implementation_name, implementation_version = implementation_name_and_version()

    has_ensurepip = True
    try:
        import ensurepip  # noqa: F401
    except ImportError:
        has_ensurepip = False

    pypy_version = cast(
        "Optional[Tuple[int, int, int]]",
        tuple(getattr(sys, "pypy_version_info", ())[:3]) or None,
    )

    sys_config_vars = sysconfig.get_config_vars()

    macos_framework_build = bool(sys_config_vars.get("PYTHONFRAMEWORK"))

    free_threaded = None  # type: Optional[bool]
    if pypy_version is None:
        free_threaded = (
            sys.version_info[:2] >= (3, 13) and sys_config_vars.get("Py_GIL_DISABLED", 0) == 1
        )

    return {
        "path": sys.executable,
        "realpath": os.path.realpath(sys.executable),
        "prefix": sys.prefix,
        "base_prefix": getattr(sys, "base_prefix", None),
        "version": {
            "major": sys.version_info.major,
            "minor": sys.version_info.minor,
            "micro": sys.version_info.micro,
            "releaselevel": sys.version_info.releaselevel,
            "serial": sys.version_info.serial,
        },
        "pypy_version": pypy_version,
        # See: https://packaging.python.org/en/latest/specifications/dependency-specifiers/#environment-markers
        "marker_env": {
            "os_name": os.name,
            "sys_platform": sys.platform,
            "platform_machine": platform.machine(),
            "platform_python_implementation": platform.python_implementation(),
            "platform_release": platform.release(),
            "platform_system": platform.system(),
            "platform_version": platform.version(),
            "python_version": ".".join(platform.python_version_tuple()[:2]),
            "python_full_version": platform.python_version(),
            "implementation_name": implementation_name,
            "implementation_version": implementation_version,
        },
        "supported_tags": ["-".join(tag) for tag in supported_tags],
        "macos_framework_build": macos_framework_build,
        "has_ensurepip": has_ensurepip,
        "free_threaded": free_threaded,
    }


def iter_generic_platform_tags():
    # type: () -> Iterator[str]

    yield normalize_string(sysconfig.get_platform())


_32_BIT_INTERPRETER = struct.calcsize("P") == 4


def mac_arch(arch):
    # type: (str) -> str

    if not _32_BIT_INTERPRETER:
        return arch

    if arch.startswith("ppc"):
        return "ppc"

    return "i386"


def mac_binary_formats(
    version,  # type: Tuple[int, int]
    cpu_arch,  # type: str
):
    # type: (...) -> List[str]

    formats = [cpu_arch]
    if cpu_arch == "x86_64":
        if version < (10, 4):
            return []
        formats.extend(["intel", "fat64", "fat32"])
    elif cpu_arch == "i386":
        if version < (10, 4):
            return []
        formats.extend(["intel", "fat32", "fat"])
    elif cpu_arch == "ppc64":
        # TODO: Need to care about 32-bit PPC for ppc64 through 10.2?
        if version > (10, 5) or version < (10, 4):
            return []
        formats.append("fat64")
    elif cpu_arch == "ppc":
        if version > (10, 6):
            return []
        formats.extend(["fat32", "fat"])

    if cpu_arch in {"arm64", "x86_64"}:
        formats.append("universal2")

    if cpu_arch in {"x86_64", "i386", "ppc64", "ppc", "intel"}:
        formats.append("universal")

    return formats


def iter_macos_platform_tags():
    # type: () -> Iterator[str]
    """Yields the platform tags for a macOS system."""
    version_str, _, cpu_arch = platform.mac_ver()

    version = tuple(map(int, version_str.split(".")[:2]))
    if version == (10, 16):
        # When built against an older macOS SDK, Python will report macOS 10.16
        # instead of the real version.
        version_str_bytes = subprocess.check_output(
            [
                sys.executable,
                "-sS",
                "-c",
                "import platform; print(platform.mac_ver()[0])",
            ],
            env={"SYSTEM_VERSION_COMPAT": "0"},
        )
        version = tuple(map(int, version_str_bytes.split(b".")[:2]))

    arch = mac_arch(cpu_arch)

    if (10, 0) <= version < (11, 0):
        # Prior to Mac OS 11, each yearly release of Mac OS bumped the
        # "minor" version number.  The major version was always 10.
        major_version = 10
        for minor_version in range(version[1], -1, -1):
            compat_version = major_version, minor_version
            binary_formats = mac_binary_formats(compat_version, arch)
            for binary_format in binary_formats:
                yield "macosx_{major_version}_{minor_version}_{binary_format}".format(
                    major_version=major_version,
                    minor_version=minor_version,
                    binary_format=binary_format,
                )

    if version >= (11, 0):
        # Starting with Mac OS 11, each yearly release bumps the major version
        # number.   The minor versions are now the midyear updates.
        minor_version = 0
        for major_version in range(version[0], 10, -1):
            compat_version = major_version, minor_version
            binary_formats = mac_binary_formats(compat_version, arch)
            for binary_format in binary_formats:
                yield "macosx_{major_version}_{minor_version}_{binary_format}".format(
                    major_version=major_version,
                    minor_version=minor_version,
                    binary_format=binary_format,
                )

    if version >= (11, 0):
        # Mac OS 11 on x86_64 is compatible with binaries from previous releases.
        # Arm64 support was introduced in 11.0, so no Arm binaries from previous
        # releases exist.
        #
        # However, the "universal2" binary format can have a
        # macOS version earlier than 11.0 when the x86_64 part of the binary supports
        # that version of macOS.
        major_version = 10
        if arch == "x86_64":
            for minor_version in range(16, 3, -1):
                compat_version = major_version, minor_version
                binary_formats = mac_binary_formats(compat_version, arch)
                for binary_format in binary_formats:
                    yield "macosx_{major_version}_{minor_version}_{binary_format}".format(
                        major_version=major_version,
                        minor_version=minor_version,
                        binary_format=binary_format,
                    )
        else:
            for minor_version in range(16, 3, -1):
                binary_format = "universal2"
                yield "macosx_{major_version}_{minor_version}_{binary_format}".format(
                    major_version=major_version,
                    minor_version=minor_version,
                    binary_format=binary_format,
                )


def iter_ios_platform_tags():
    # type: () -> Iterator[str]
    """Yields the platform tags for an iOS system."""

    # if iOS is the current platform, ios_ver *must* be defined. However,
    # it won't exist for CPython versions before 3.13, which causes a mypy
    # error.
    _, release, _, _ = platform.ios_ver()  # type: ignore[attr-defined, unused-ignore]
    version = tuple(map(int, release.split(".")[:2]))

    # If the requested major version is less than 12, there won't be any matches.
    if version[0] < 12:
        return

    # N.B.: `sys.implementation` is always defined for Python 3.12 and newer.
    multiarch = sys.implementation._multiarch.replace("-", "_")  # type: ignore[attr-defined]

    ios_platform_template = "ios_{major}_{minor}_{multiarch}"

    # Consider any iOS major.minor version from the version requested, down to
    # 12.0. 12.0 is the first iOS version that is known to have enough features
    # to support CPython. Consider every possible minor release up to X.9. There
    # highest the minor has ever gone is 8 (14.8 and 15.8) but having some extra
    # candidates that won't ever match doesn't really hurt, and it saves us from
    # having to keep an explicit list of known iOS versions in the code. Return
    # the results descending order of version number.

    # Consider the actual X.Y version that was requested.
    yield ios_platform_template.format(major=version[0], minor=version[1], multiarch=multiarch)

    # Consider every minor version from X.0 to the minor version prior to the
    # version requested by the platform.
    for minor in range(version[1] - 1, -1, -1):
        yield ios_platform_template.format(major=version[0], minor=minor, multiarch=multiarch)

    for major in range(version[0] - 1, 11, -1):
        for minor in range(9, -1, -1):
            yield ios_platform_template.format(major=major, minor=minor, multiarch=multiarch)


def iter_android_platform_tags():
    # type: () -> Iterator[str]
    """Yields the :attr:`~Tag.platform` tags for Android."""
    # Python 3.13 was the first version to return platform.system() == "Android",
    # and also the first version to define platform.android_ver().
    api_level = platform.android_ver().api_level  # type: ignore[attr-defined]

    abi = normalize_string(sysconfig.get_platform().split("-")[-1])

    # 16 is the minimum API level known to have enough features to support CPython
    # without major patching. Yield every API level from the maximum down to the
    # minimum, inclusive.
    min_api_level = 16
    for ver in range(api_level, min_api_level - 1, -1):
        yield "android_{ver}_{abi}".format(ver=ver, abi=abi)


def iter_linux_platform_tags(linux_info):
    # type: (Dict[str, Any]) -> Iterator[str]

    linux = normalize_string(sysconfig.get_platform())
    if not linux.startswith("linux_"):
        # we should never be here, just yield the sysconfig one and return
        yield linux
        return

    if _32_BIT_INTERPRETER:
        if linux == "linux_x86_64":
            linux = "linux_i686"
        elif linux == "linux_aarch64":
            linux = "linux_armv8l"

    _, arch = linux.split("_", 1)
    arches = {"armv8l": ["armv8l", "armv7l"]}.get(arch, [arch])

    manylinux = linux_info.get("manylinux")
    if manylinux:
        glibc = manylinux["glibc"]
        glibc_version = (int(glibc["major"]), int(glibc["minor"])) if glibc else get_glibc_version()
        armhf = bool(manylinux["armhf"])
        i686 = bool(manylinux["i686"])
        for tag in iter_manylinux_platform_tags(  # noqa: E731
            current_glibc=glibc_version, armhf=armhf, i686=i686, arches=arches
        ):
            yield tag
    else:
        musllinux = linux_info["musllinux"]
        musl_version = int(musllinux["major"]), int(musllinux["minor"])
        for tag in iter_musllinux_platform_tags(musl_version, arches):  # noqa: E731
            yield tag

    for arch in arches:
        yield "linux_{arch}".format(arch=arch)


def current_arch():
    # type: () -> str

    plat = sysconfig.get_platform()
    return re.sub(r"[.-]", "_", plat.split("-", 1)[-1])


def glibc_version_string_confstr():
    # type: () -> Optional[str]
    """Primary implementation of glibc_version_string using os.confstr."""
    # os.confstr is quite a bit faster than ctypes.DLL. It's also less likely
    # to be broken or missing. This strategy is used in the standard library
    # platform module.
    # https://github.com/python/cpython/blob/fcf1d003bf4f0100c/Lib/platform.py#L175-L183
    try:
        # Should be a string like "glibc 2.17".
        # N.B.: os.confstr is not a defined attribute on Windows.
        version_string = os.confstr("CS_GNU_LIBC_VERSION")  # type: ignore[attr-defined]
        assert version_string is not None
        _, version = version_string.rsplit()
    except (AssertionError, AttributeError, OSError, ValueError):
        # os.confstr() or CS_GNU_LIBC_VERSION not available (or a bad value)...
        return None
    return version


def glibc_version_string_ctypes():
    # type: () -> Optional[str]
    """
    Fallback implementation of glibc_version_string using ctypes.
    """
    try:
        import ctypes
    except ImportError:
        return None

    # ctypes.CDLL(None) internally calls dlopen(NULL), and as the dlopen
    # manpage says, "If filename is NULL, then the returned handle is for the
    # main program". This way we can let the linker do the work to figure out
    # which libc our process is actually using.
    #
    # We must also handle the special case where the executable is not a
    # dynamically linked executable. This can occur when using musl libc,
    # for example. In this situation, dlopen() will error, leading to an
    # OSError. Interestingly, at least in the case of musl, there is no
    # errno set on the OSError. The single string argument used to construct
    # OSError comes from libc itself and is therefore not portable to
    # hard code here. In any case, failure to call dlopen() means we
    # can proceed, so we bail on our attempt.
    try:
        process_namespace = ctypes.CDLL(None)
    except OSError:
        return None

    try:
        gnu_get_libc_version = process_namespace.gnu_get_libc_version
    except AttributeError:
        # Symbol doesn't exist -> therefore, we are not linked to
        # glibc.
        return None

    # Call gnu_get_libc_version, which returns a string like "2.5"
    gnu_get_libc_version.restype = ctypes.c_char_p
    version_str = gnu_get_libc_version()
    # py2 / py3 compatibility:
    if not isinstance(version_str, str):
        version_str = version_str.decode("ascii")

    return version_str


def glibc_version_string():
    # type: () -> Optional[str]
    """Returns glibc version string, or None if not using glibc."""
    return glibc_version_string_confstr() or glibc_version_string_ctypes()


def parse_glibc_version(version_str):
    # type: (str) -> Tuple[int, int]
    """Parse glibc version.

    We use a regexp instead of str.split because we want to discard any
    random junk that might come after the minor version -- this might happen
    in patched/forked versions of glibc (e.g. Linaro's version of glibc
    uses version strings like "2.20-2014.11"). See gh-3588.
    """
    m = re.match(r"(?P<major>[0-9]+)\.(?P<minor>[0-9]+)", version_str)
    if not m:
        return -1, -1
    return int(m.group("major")), int(m.group("minor"))


def get_glibc_version():
    # type: () -> Tuple[int, int]

    version_str = glibc_version_string()
    if version_str is None:
        return -1, -1
    return parse_glibc_version(version_str)


# If glibc ever changes its major version, we need to know what the last
# minor version was, so we can build the complete list of all versions.
# For now, guess what the highest minor version might be, assume it will
# be 50 for testing. Once this actually happens, update the dictionary
# with the actual value.
_LAST_GLIBC_MINOR = collections.defaultdict(lambda: 50)  # type: DefaultDict[int, int]


# From PEP 513, PEP 600
def is_glibc_version_compatible(
    arch,  # type: str
    sys_glibc,  # type: Tuple[int, int]
    version,  # type: Tuple[int, int]
):
    # type: (...) -> bool

    if sys_glibc < version:
        return False
    # Check for presence of _manylinux module.
    try:
        import _manylinux  # type: ignore
    except ImportError:
        return True
    if hasattr(_manylinux, "manylinux_compatible"):
        result = _manylinux.manylinux_compatible(version[0], version[1], arch)
        if result is not None:
            return bool(result)
        return True
    if version == (2, 5):
        if hasattr(_manylinux, "manylinux1_compatible"):
            return bool(_manylinux.manylinux1_compatible)
    if version == (2, 12):
        if hasattr(_manylinux, "manylinux2010_compatible"):
            return bool(_manylinux.manylinux2010_compatible)
    if version == (2, 17):
        if hasattr(_manylinux, "manylinux2014_compatible"):
            return bool(_manylinux.manylinux2014_compatible)
    return True


_LEGACY_MANYLINUX_MAP = {
    # CentOS 7 w/ glibc 2.17 (PEP 599)
    (2, 17): "manylinux2014",
    # CentOS 6 w/ glibc 2.12 (PEP 571)
    (2, 12): "manylinux2010",
    # CentOS 5 w/ glibc 2.5 (PEP 513)
    (2, 5): "manylinux1",
}

_MAJOR = 0
_MINOR = 1


def have_compatible_abi(
    arches,  # type: Sequence[str]
    armhf,  # type: bool
    i686,  # type: bool
):
    # type: (...) -> bool

    if "armv7l" in arches:
        return armhf
    if "i686" in arches:
        return i686
    allowed_arches = {
        "x86_64",
        "aarch64",
        "ppc64",
        "ppc64le",
        "s390x",
        "loongarch64",
        "riscv64",
    }
    return any(arch in allowed_arches for arch in arches)


def iter_manylinux_platform_tags(
    current_glibc,  # type: Tuple[int, int]
    armhf,  # type: bool
    i686,  # type: bool
    arches,  # type: Sequence[str]
):
    # type: (...) -> Iterator[str]

    if not have_compatible_abi(arches, armhf, i686):
        return

    # Oldest glibc to be supported regardless of architecture is (2, 17).
    too_old_glibc2 = 2, 16
    if set(arches) & {"x86_64", "i686"}:
        # On x86/i686 also oldest glibc to be supported is (2, 5).
        too_old_glibc2 = 2, 4

    glibc_max_list = [current_glibc]
    # We can assume compatibility across glibc major versions.
    # https://sourceware.org/bugzilla/show_bug.cgi?id=24636
    #
    # Build a list of maximum glibc versions so that we can
    # output the canonical list of all glibc from current_glibc
    # down to too_old_glibc2, including all intermediary versions.
    for glibc_major in range(current_glibc[_MAJOR] - 1, 1, -1):
        glibc_minor = _LAST_GLIBC_MINOR[glibc_major]
        glibc_max_list.append((glibc_major, glibc_minor))
    for arch in arches:
        for glibc_max in glibc_max_list:
            if glibc_max[_MAJOR] == too_old_glibc2[_MAJOR]:
                min_minor = too_old_glibc2[_MINOR]
            else:
                # For other glibc major versions the oldest supported is (x, 0).
                min_minor = -1
            for glibc_minor in range(glibc_max[_MINOR], min_minor, -1):
                glibc_version = (glibc_max[_MAJOR], glibc_minor)
                tag = "manylinux_{}_{}".format(*glibc_version)
                if is_glibc_version_compatible(arch, current_glibc, glibc_version):
                    yield "{tag}_{arch}".format(tag=tag, arch=arch)
                # Handle the legacy manylinux1, manylinux2010, manylinux2014 tags.
                if glibc_version in _LEGACY_MANYLINUX_MAP:
                    legacy_tag = _LEGACY_MANYLINUX_MAP[glibc_version]
                    if is_glibc_version_compatible(arch, current_glibc, glibc_version):
                        yield "{legacy_tag}_{arch}".format(legacy_tag=legacy_tag, arch=arch)


def iter_musllinux_platform_tags(
    version,  # type: Tuple[int, int]
    arches,  # type: Sequence[str]
):
    # type: (...) -> Iterator[str]

    major, minor = version
    for arch in arches:
        for minor in range(minor, -1, -1):
            yield "musllinux_{major}_{minor}_{arch}".format(major=major, minor=minor, arch=arch)


INTERPRETER_SHORT_NAMES = {
    "python": "py",  # Generic.
    "cpython": "cp",
    "pypy": "pp",
    "ironpython": "ip",
    "jython": "jy",
}


def get_config_var(name):
    # type: (str) -> Optional[Union[int, str]]

    return sysconfig.get_config_vars().get(name)


def interpreter_version():
    # type: () -> str
    """
    Returns the version of the running interpreter.
    """
    version = get_config_var("py_version_nodot")
    if version:
        version = str(version)
    else:
        version = version_nodot(sys.version_info[:2])
    return version


def normalize_string(string):
    # type: (str) -> str

    return string.replace(".", "_").replace("-", "_").replace(" ", "_")


def is_threaded_cpython(abis):
    # type: (List[str]) -> bool
    """
    Determine if the ABI corresponds to a threaded (`--disable-gil`) build.

    The threaded builds are indicated by a "t" in the abiflags.
    """
    if len(abis) == 0:
        return False
    # expect e.g., cp313
    m = re.match(r"cp\d+(.*)", abis[0])
    if not m:
        return False
    abiflags = m.group(1)
    return "t" in abiflags


def abi3_applies(
    python_version,  # type: Sequence[int]
    threading,  # type: bool
):
    # type: (...) -> bool
    """
    Determine if the Python version supports abi3.

    PEP 384 was first implemented in Python 3.2. The threaded (`--disable-gil`)
    builds do not support abi3.
    """
    return len(python_version) > 1 and tuple(python_version) >= (3, 2) and not threading


def cpython_abis(py_version):
    # type: (Sequence[int]) -> List[str]

    try:
        # N.B.: There is no importlib.machinery prior to ~3.3.
        from importlib.machinery import EXTENSION_SUFFIXES  # type: ignore[import-not-found]
    except ImportError:
        # N.B.: There is no imp from 3.12 on.
        import imp  # type: ignore[import-not-found]

        EXTENSION_SUFFIXES = [x[0] for x in imp.get_suffixes()]
        del imp

    py_version = tuple(py_version)  # To allow for version comparison.
    abis = []
    version = version_nodot(py_version[:2])
    threading = debug = pymalloc = ucs4 = ""
    with_debug = get_config_var("Py_DEBUG")
    has_refcount = hasattr(sys, "gettotalrefcount")
    # Windows doesn't set Py_DEBUG, so checking for support of debug-compiled
    # extension modules is the best option.
    # https://github.com/pypa/pip/issues/3383#issuecomment-173267692
    has_ext = "_d.pyd" in EXTENSION_SUFFIXES
    if with_debug or (with_debug is None and (has_refcount or has_ext)):
        debug = "d"
    if py_version >= (3, 13) and get_config_var("Py_GIL_DISABLED"):
        threading = "t"
    if py_version < (3, 8):
        with_pymalloc = get_config_var("WITH_PYMALLOC")
        if with_pymalloc or with_pymalloc is None:
            pymalloc = "m"
        if py_version < (3, 3):
            unicode_size = get_config_var("Py_UNICODE_SIZE")
            if unicode_size == 4 or (unicode_size is None and sys.maxunicode == 0x10FFFF):
                ucs4 = "u"
    elif debug:
        # Debug builds can also load "normal" extension modules.
        # We can also assume no UCS-4 or pymalloc requirement.
        abis.append("cp{version}{threading}".format(version=version, threading=threading))
    abis.insert(
        0,
        "cp{version}{threading}{debug}{pymalloc}{ucs4}".format(
            version=version, threading=threading, debug=debug, pymalloc=pymalloc, ucs4=ucs4
        ),
    )
    return abis


def generic_abi():
    # type: () -> List[str]
    """
    Return the ABI tag based on EXT_SUFFIX.
    """
    # The following are examples of `EXT_SUFFIX`.
    # We want to keep the parts which are related to the ABI and remove the
    # parts which are related to the platform:
    # - linux:   '.cpython-310-x86_64-linux-gnu.so' => cp310
    # - mac:     '.cpython-310-darwin.so'           => cp310
    # - win:     '.cp310-win_amd64.pyd'             => cp310
    # - win:     '.pyd'                             => cp37 (uses cpython_abis())
    # - pypy:    '.pypy38-pp73-x86_64-linux-gnu.so' => pypy38_pp73
    # - graalpy: '.graalpy-38-native-x86_64-darwin.dylib'
    #                                               => graalpy_38_native

    ext_suffix = get_config_var("EXT_SUFFIX") or get_config_var("SO")
    if not isinstance(ext_suffix, str) or ext_suffix[0] != ".":
        raise SystemError("invalid sysconfig.get_config_var('EXT_SUFFIX')")
    parts = ext_suffix.split(".")
    if len(parts) < 3:
        # CPython3.7 and earlier uses ".pyd" on Windows.
        return cpython_abis(sys.version_info[:2])
    soabi = parts[1]
    if soabi.startswith("cpython"):
        # non-windows
        abi = "cp" + soabi.split("-")[1]
    elif soabi.startswith("cp"):
        # windows
        abi = soabi.split("-")[0]
    elif soabi.startswith("pypy"):
        abi = "-".join(soabi.split("-")[:2])
    elif soabi.startswith("graalpy"):
        abi = "-".join(soabi.split("-")[:3])
    elif soabi:
        # pyston, ironpython, others?
        abi = soabi
    else:
        return []
    return [normalize_string(abi)]


def interpreter_name():
    # type: () -> str
    """
    Returns the name of the running interpreter.

    Some implementations have a reserved, two-letter abbreviation which will
    be returned when appropriate.
    """
    long_name = (
        sys.implementation.name  # type: ignore[attr-defined]
        if hasattr(sys, "implementation")
        else platform.python_implementation().lower()
    )
    return INTERPRETER_SHORT_NAMES.get(long_name, long_name)


def cpython_tags(platforms):
    # type: (Iterable[str]) -> Iterator[Tuple[str, str, str]]
    """Yields the tags for a CPython interpreter.

    The tags consist of:
    - cp<python_version>-<abi>-<platform>
    - cp<python_version>-abi3-<platform>
    - cp<python_version>-none-<platform>
    - cp<less than python_version>-abi3-<platform>  # Older Python versions down to 3.2.
    """
    python_version = sys.version_info[:2]

    interpreter = "cp{version_nodot}".format(version_nodot=version_nodot(python_version[:2]))

    if len(python_version) > 1:
        abis = cpython_abis(python_version)
    else:
        abis = []

    abis = list(abis)
    # 'abi3' and 'none' are explicitly handled later.
    for explicit_abi in ("abi3", "none"):
        try:
            abis.remove(explicit_abi)
        except ValueError:
            pass

    for abi in abis:
        for platform_ in platforms:
            yield interpreter, abi, platform_

    threading = is_threaded_cpython(abis)
    use_abi3 = abi3_applies(python_version, threading)
    if use_abi3:
        for platform_ in platforms:
            yield interpreter, "abi3", platform_
    for platform_ in platforms:
        yield interpreter, "none", platform_

    if use_abi3:
        for minor_version in range(python_version[1] - 1, 1, -1):
            for platform_ in platforms:
                version = version_nodot((python_version[0], minor_version))
                interpreter = "cp{version}".format(version=version)
                yield interpreter, "abi3", platform_


def generic_tags(platforms):
    # type: (Iterable[str]) -> Iterator[Tuple[str, str, str]]
    """
    Yields the tags for a generic interpreter.

    The tags consist of:
    - <interpreter>-<abi>-<platform>

    The "none" ABI will be added if it was not explicitly provided.
    """

    interp_name = interpreter_name()
    interp_version = interpreter_version()
    interpreter = "".join([interp_name, interp_version])

    abis = generic_abi()
    if "none" not in abis:
        abis.append("none")

    for abi in abis:
        for platform_ in platforms:
            yield interpreter, abi, platform_


def version_nodot(version):
    # type: (Sequence[int]) -> str

    return "".join(map(str, version))


def py_interpreter_range(py_version):
    # type: (Sequence[int]) -> Iterator[str]
    """
    Yields Python versions in descending order.

    After the latest version, the major-only version will be yielded, and then
    all previous versions of that major version.
    """
    if len(py_version) > 1:
        yield "py{major_minor}".format(major_minor=version_nodot(py_version[:2]))
    yield "py{major}".format(major=py_version[0])
    if len(py_version) > 1:
        for minor in range(py_version[1] - 1, -1, -1):
            yield "py{major_minor}".format(major_minor=version_nodot((py_version[0], minor)))


def compatible_tags(
    platforms,  # type: Iterable[str]
    interpreter=None,  # type: Optional[str]
):
    # type: (...) -> Iterator[Tuple[str, str, str]]
    """
    Yields the sequence of tags that are compatible with a specific version of Python.

    The tags consist of:
    - py*-none-<platform>
    - <interpreter>-none-any  # ... if `interpreter` is provided.
    - py*-none-any
    """
    python_version = sys.version_info[:2]
    for version in py_interpreter_range(python_version):
        for platform_ in platforms:
            yield version, "none", platform_
    if interpreter:
        yield interpreter, "none", "any"
    for version in py_interpreter_range(python_version):
        yield version, "none", "any"


def iter_supported_tags(platforms):
    # type: (Tuple[str, ...]) -> Iterator[Tuple[str, str, str]]

    interp_name = interpreter_name()
    if interp_name == "cp":
        for tag in cpython_tags(platforms):
            yield tag
    else:
        for tag in generic_tags(platforms):
            yield tag

    if interp_name == "pp" and sys.version_info[0] == 3:
        interp = "pp3"
    elif interp_name == "cp":
        interp = "cp" + interpreter_version()
    else:
        interp = None
    for tag in compatible_tags(platforms, interpreter=interp):
        yield tag


OS = platform.system().lower()
IS_ANDROID = OS == "android"
IS_IOS = OS == "ios"
IS_LINUX = OS == "linux"
IS_MAC = OS == "darwin"


def main():
    # type: () -> Any

    parser = ArgumentParser(prog="interpreter.py")
    parser.add_argument("output_path", nargs="?", default=None)
    if IS_LINUX:
        parser.add_argument("--linux-info", metavar="JSON", required=True)
    options = parser.parse_args()

    @contextmanager
    def output(file_path=None):
        # type: (Optional[str]) -> Iterator[IO[str]]
        if file_path is None:
            yield sys.stdout
        else:
            with open(file_path, "w") as fp:
                yield fp

    path = options.output_path  # type: Optional[str]
    if IS_ANDROID:
        iter_supported_platform_tags = iter_android_platform_tags
    elif IS_IOS:
        iter_supported_platform_tags = iter_ios_platform_tags
    elif IS_MAC:
        iter_supported_platform_tags = iter_macos_platform_tags
    elif IS_LINUX:
        linux_info = json.loads(options.linux_info)
        iter_supported_platform_tags = functools.partial(iter_linux_platform_tags, linux_info)
    else:
        iter_supported_platform_tags = iter_generic_platform_tags

    with output(file_path=path) as out:
        json.dump(identify(list(iter_supported_tags(tuple(iter_supported_platform_tags())))), out)


if __name__ == "__main__":
    sys.exit(main())
