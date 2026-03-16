# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import os.path
import subprocess
from textwrap import dedent

from testing import skip_windows_cant_build_pex_to_inject_yet
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


@skip_windows_cant_build_pex_to_inject_yet
def test_non_hermetic(tmpdir):
    # type: (Any) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    pex = os.path.join(str(tmpdir), "pex")
    exe = os.path.join(str(tmpdir), "exe.py")
    with open(exe, "w") as fp:
        fp.write(
            dedent(
                """\
                import os


                def load_data():
                    with open(os.environ["ADJOINED"]) as fp:
                        return fp.read()


                if __name__ == "__main__":
                    print(load_data(), end="")
                """
            )
        )
    subprocess.check_call(
        args=[
            "pex",
            "--runtime-pex-root",
            pex_root,
            "--exe",
            exe,
            "--bind-resource-path",
            "ADJOINED=data/file",
            "--inherit-path",
            "append",
            "-o",
            pex,
        ]
    )

    adjoined = os.path.join(str(tmpdir), "adjoined-sys-path")
    data_file = os.path.join(adjoined, "data", "file")
    os.makedirs(os.path.dirname(data_file))
    with open(data_file, "w") as fp:
        fp.write("42")

    def test_result(
        result,  # type: ProcessResult
        _is_traditional_pex,  # type: bool
    ):
        # type: (...) -> None
        if _is_traditional_pex:
            return
        result.assert_success()
        assert "42" == result.stdout

    compare(
        pex,
        env=dict(PYTHONPATH=adjoined, PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=test_result,
    )
