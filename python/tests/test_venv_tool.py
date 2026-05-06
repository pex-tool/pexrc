# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess
import sys

import pytest
from testing import pexrc_inject
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Iterable, Optional, Text, Tuple  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


@pytest.fixture
def cowsay_pex(tmpdir):
    # type: (Any) -> str

    src = os.path.join(str(tmpdir), "src")
    pex_root = os.path.join(str(tmpdir), "pex-root")
    cowsay = os.path.join(src, "cowsay")
    os.makedirs(cowsay)
    with open(os.path.join(cowsay, "__init__.py"), "w") as fp:
        fp.write("from .main import tux")

    pex = os.path.join(str(tmpdir), "cowsay.pex")
    subprocess.check_call(
        args=[
            "pex",
            "--runtime-pex-root",
            pex_root,
            "cowsay==5",
            "-P",
            "cowsay@{src}".format(src=src),
            "-o",
            pex,
            "--emit-warnings",
            "--venv",
            "--venv-site-packages-copies",
        ]
    )
    return pex


def run_venv_tool(
    tmpdir,  # type: Any
    pex,  # type: str
    venv_tool_options=(),  # type: Iterable[str]
):
    # type: (...) -> Tuple[str, int, Text]

    env = os.environ.copy()
    env.update(PEX_TOOLS="1")
    venv_dir = os.path.join(str(tmpdir), "venv")
    process = subprocess.Popen(
        args=[sys.executable, pex, "venv", venv_dir] + list(venv_tool_options),
        env=env,
        stderr=subprocess.PIPE,
    )
    _, stderr = process.communicate()
    return venv_dir, process.returncode, stderr.decode("utf-8")


def assert_collisions(
    pex,  # type: str
    output,  # type: Text
    venv_dir=None,  # type: Optional[str]
):
    # type: (...) -> None

    if venv_dir:
        assert (
            "While populating venv at {venv_dir} for {pex} encountered 1 collision:".format(
                venv_dir=venv_dir, pex=pex
            )
            in output
        )
    else:
        assert "While populating venv for {pex} encountered 1 collision:".format(pex=pex) in output

    assert "Had 2 distinct sources for " in output
    assert "1. 4eb8cc977b02aae6e675b51800787257eaeb164d32343f726218672d09e3ba28 390 bytes" in output
    assert "2. 5b3e817326aec530e9cea0a83220b85acf7fff9d57df4c7e2aba0afc58d8b1e9 21 bytes" in output


def test_collision_warn(
    tmpdir,  # type: Any
    cowsay_pex,  # type: str
    pexrc_root,  # type: str
):
    # type: (...) -> None

    def test_result(
        result,  # type: ProcessResult
        is_traditional_pex,  # type: bool
    ):
        # type: (...) -> None
        result.assert_success()
        if not is_traditional_pex:
            assert_collisions(pex=result.pex, output=result.stderr)

    injected_pex = compare(
        cowsay_pex,
        args=["-c", "import cowsay; cowsay.tux('Moo?')"],
        env=dict(PEXRC_ROOT=pexrc_root),
        test_result=test_result,
    )
    venv_dir, returncode, stderr = run_venv_tool(
        tmpdir, injected_pex, venv_tool_options=["--collisions-ok"]
    )
    assert returncode == 0
    assert_collisions(pex=injected_pex, output=stderr, venv_dir=venv_dir)


def test_collision_error(
    tmpdir,  # type: Any
    cowsay_pex,  # type: str
):
    # type: (...) -> None

    injected_pex = pexrc_inject(cowsay_pex)
    venv_dir, returncode, stderr = run_venv_tool(tmpdir, injected_pex)
    assert returncode != 0
    assert_collisions(pex=injected_pex, output=stderr, venv_dir=venv_dir)
