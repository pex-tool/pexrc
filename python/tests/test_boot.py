# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess

from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def create_cowsay_pex(
    tmpdir,  # type: Any
    *pex_args,  # type: str
):
    pex = os.path.join(str(tmpdir), "cowsay.pex")
    pex_root = os.path.join(str(tmpdir), "pex-root")
    subprocess.check_call(
        args=[
            "pex",
            "cowsay<6",
            "-c",
            "cowsay",
            "-o",
            pex,
            "--runtime-pex-root",
            pex_root,
        ]
        + list(pex_args)
    )
    return pex


def assert_result(
    result,  # type: ProcessResult
    _is_traditional_pex,  # type: bool
):
    # type: (...) -> None
    result.assert_success()
    assert "| Moo! |" in result.stdout


def read_shebang(pex):
    # type: (str) -> Text

    with open(pex, "rb") as fp:
        return fp.readline().decode("utf-8")


def test_basic(tmpdir):
    # type: (Any) -> None

    pex = create_cowsay_pex(tmpdir)
    expected_shebang = read_shebang(pex)
    assert expected_shebang.startswith("#!/usr/bin/env ")

    injected_pex = compare(
        pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=assert_result,
    )
    assert expected_shebang == read_shebang(injected_pex)


def test_sh_boot(tmpdir):
    # type: (Any) -> None

    pex = create_cowsay_pex(tmpdir, "--sh-boot")
    expected_shebang = read_shebang(pex)
    assert expected_shebang == "#!/bin/sh\n"

    injected_pex = compare(
        pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=assert_result,
    )
    assert expected_shebang == read_shebang(injected_pex)
