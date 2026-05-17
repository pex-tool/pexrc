# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess

import pytest
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, List, Text  # noqa: F401

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


@pytest.mark.parametrize(
    "layout_args",
    [pytest.param(["--layout", layout], id=layout) for layout in ("zipapp", "loose", "packed")],
)
@pytest.mark.parametrize(
    "deps_are_wheel_files_args",
    [pytest.param(["--no-pre-install-wheels"], id="whls"), pytest.param([], id="chroots")],
)
@pytest.mark.parametrize(
    "boot_args", [pytest.param(["--sh-boot"], id="sh-boot"), pytest.param([], id="py-boot")]
)
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
    )
