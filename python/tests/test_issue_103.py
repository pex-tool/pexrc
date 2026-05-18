# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess

import pytest
from testing import IS_MAC, IS_WINDOWS
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Iterator, List, Text, Tuple  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def create_sqlalchemy_pex(
    tmpdir,  # type: Any
    extra_pex_args,  # type: List[str]
):
    # type: (...) -> str

    venv = os.path.join(str(tmpdir), "venv")
    subprocess.check_call(args=["uv", "venv", "--no-project", venv])
    subprocess.check_call(args=["uv", "pip", "install", "--python", venv, "sqlalchemy"])

    pex_root = os.path.join(str(tmpdir), "pex-root")
    pex = os.path.join(str(tmpdir), "pex")
    subprocess.check_call(
        args=[
            "pex",
            "--runtime-pex-root",
            pex_root,
            "--venv-repository",
            venv,
            "sqlalchemy",
            "-o",
            pex,
        ]
        + extra_pex_args
    )
    return pex


def boot_args():
    # type: () -> Iterator[Tuple[List[str]]]
    if not IS_WINDOWS:
        # TODO: Re-enable --sh-boot for Windows.
        #  N.B.: --sh-boot doesn't work on Windows; worse it actively foils `python ./pex.rc` due
        #  to embeds of Windows-style paths in the --sh-boot script currently:
        #  > #!/bin/sh
        #  > '''': pshprs
        #  > # N.B.: This script should stick to syntax defined for POSIX `sh` and avoid non-builtins.
        #  > # See: https://pubs.opengroup.org/onlinepubs/9699919799/idx/shell.html
        #  > set -eu
        #  >
        #  > VENV=""
        #  > VENV_PYTHON_ARGS="-I"
        #  >
        #  > # N.B.: This ensures tilde-expansion of the DEFAULT_PEX_ROOT value.
        #  > DEFAULT_PEX_ROOT="$(echo C:\Users\jsirois\AppData\Local\Temp\pytest-of-jsirois\pytest-52\test_issue_103_sh_boot_whls_pa0\pex-root)"
        #  >
        #  > DEFAULT_PYTHON=""
        #  > PYTHON_ARGS=""
        #  >
        #  > PEX_ROOT="${PEX_ROOT:-${DEFAULT_PEX_ROOT}}"
        #  > INSTALLED_PEX="${PEX_ROOT}/unzipped_pexes\3\d8d0b156dde66d6862e092fc7b81ba3e5177dc20"
        yield pytest.param(["--sh-boot"], id="sh-boot")
    yield pytest.param([], id="py-boot")


def deps_are_wheel_files_args():
    # type: () -> Iterator[Tuple[List[str]]]
    if not IS_WINDOWS:
        # TODO: Re-enable --no-pre-install-wheels for Windows.
        #  N.B.: Pex fails to build a PEX with --no-pre-install-wheels on Windows:
        #  E         File "C:\Users\jsirois\AppData\Local\Temp\pytest-of-jsirois\pytest-53\popen-gw1\test_issue_103_py_boot_whls_lo0\pex\.bootstrap\pex\pep_376.py", line 183, in read
        #  E           for line, (path, fingerprint, file_size) in enumerate(
        #  E                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        #  E       ValueError: not enough values to unpack (expected 3, got 0)
        yield pytest.param(["--no-pre-install-wheels"], id="whls")
    yield pytest.param([], id="chroots")


@pytest.mark.parametrize(
    "layout_args",
    [pytest.param(["--layout", layout], id=layout) for layout in ("zipapp", "loose", "packed")],
)
@pytest.mark.parametrize("deps_are_wheel_files_args", list(deps_are_wheel_files_args()))
@pytest.mark.parametrize("boot_args", list(boot_args()))
def test_issue_103(
    tmpdir,  # type: Any
    pexrc_root,  # type: str
    layout_args,  # type: List[str]
    deps_are_wheel_files_args,  # type: List[str]
    boot_args,  # type: List[str]
):
    # type: (...) -> None

    compare(
        create_sqlalchemy_pex(tmpdir, layout_args + deps_are_wheel_files_args + boot_args),
        args=["-c", "import sqlalchemy"],
        env=dict(PEXRC_ROOT=pexrc_root),
        # N.B.: Mac SIP causes this 1st run of the --sh-boot variants to be unavoidably slow
        # when they select an interpreter that is not the current one.
        assert_faster=not IS_MAC,
    )
