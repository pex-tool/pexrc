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


def test_basic(tmpdir):
    # type: (Any) -> None

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
    )

    def test_result(
        result,  # type: ProcessResult
        _is_traditional_pex,  # type: bool
    ):
        # type: (...) -> None
        result.assert_success()
        assert "| Moo! |" in result.stdout

    compare(
        pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=test_result,
    )
