# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess
from textwrap import dedent

import colors  # type: ignore[import-untyped]
from testing.compare import compare, execute_pex

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def test_python_args_forwarded(
    tmpdir,  # type: Any
    pexrc_root,  # type: str
):
    # type: (...) -> None

    pex = os.path.join(str(tmpdir), "cowsay.pex")
    pex_root = os.path.join(str(tmpdir), "pex-root")

    exe = os.path.join(str(tmpdir), "exe.py")
    with open(exe, "w") as fp:
        fp.write(
            dedent(
                """\
                from __future__ import print_function

                import sys

                import colors


                assert False, colors.red("Failed")
                print(colors.green("Worked: {}".format(" ".join(sys.argv[1:]))), end="")
                """
            )
        )

    subprocess.check_call(
        args=[
            "pex",
            "ansicolors==1.1.8",
            "--exe",
            exe,
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
        assert colors.green("Worked: Slartibartfast Ford") == result.stdout

    injected_pex = compare(
        pex,
        python_args=["-O"],
        args=["Slartibartfast", "Ford"],
        env=dict(PEXRC_ROOT=pexrc_root),
        test_result=test_result,
    )

    result = execute_pex(pex)
    result.assert_failure()
    assert colors.red("Failed") in result.stderr

    result = execute_pex(injected_pex)
    result.assert_failure()
    assert colors.red("Failed") in result.stderr
