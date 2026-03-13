# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import os.path
import subprocess
from textwrap import dedent

import colors  # type: ignore[import-untyped]
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def test_via_env(tmpdir):
    # type: (Any) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    ansicolors_pex = os.path.join(str(tmpdir), "ansicolors.pex")
    subprocess.check_call(
        args=[
            "pex",
            "ansicolors==1.1.8",
            "-o",
            ansicolors_pex,
            "--runtime-pex-root",
            pex_root,
        ]
    )

    exe = os.path.join(str(tmpdir), "exe.py")
    with open(exe, "w") as fp:
        fp.write(
            dedent(
                """\
                import sys

                import colors
                import cowsay


                def dragon(message):
                    cowsay.dragon(colors.cyan(message))


                if __name__ == "__main__":
                    dragon(" ".join(sys.argv[1:]))
                """
            )
        )

    cowsay_pex = os.path.join(str(tmpdir), "cowsay.pex")
    subprocess.check_call(
        args=[
            "pex",
            "cowsay<6",
            "--exe",
            exe,
            "-o",
            cowsay_pex,
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
        assert "| {message} |".format(message=colors.cyan("Moo?")) in result.stdout

    compare(
        cowsay_pex,
        args=["Moo?"],
        env=dict(PEX_PATH=ansicolors_pex, PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=test_result,
    )
