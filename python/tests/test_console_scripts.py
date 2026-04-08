# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess
from textwrap import dedent

from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def test_venv_console_scripts(
    tmpdir,  # type: Any
    pexrc_root,  # type: str
):
    # type: (...) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    pex = os.path.join(str(tmpdir), "indirect-cowsay.pex")
    exe = os.path.join(str(tmpdir), "exe.py")
    with open(exe, "w") as fp:
        fp.write(
            dedent(
                """\
                import subprocess
                import sys


                if __name__ == "__main__":
                    sys.exit(subprocess.call(args=["cowsay", " ".join(sys.argv[1:])]))
                """
            )
        )
    subprocess.check_call(
        args=[
            "pex",
            "--runtime-pex-root",
            pex_root,
            "cowsay==5",
            "--exe",
            exe,
            "-o",
            pex,
            "--venv",
            "prepend",
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
        env=dict(PEXRC_ROOT=pexrc_root),
        test_result=test_result,
    )
