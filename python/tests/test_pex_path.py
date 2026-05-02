# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import json
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


def test_via_env(
    tmpdir,  # type: Any
    pexrc_root,  # type: str
):
    # type: (...) -> None

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
        env=dict(PEX_PATH=ansicolors_pex, PEXRC_ROOT=pexrc_root),
        test_result=test_result,
    )


def test_data_dirs(
    tmpdir,  # type: Any
    pexrc_root,  # type: str
):
    # type: (...) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    data_dirs_pex = os.path.join(str(tmpdir), "data-dirs.pex")
    subprocess.check_call(
        args=[
            "pex",
            "greenlet",
            "uWSGI",
            "-o",
            data_dirs_pex,
            "--runtime-pex-root",
            pex_root,
        ]
    )

    exe = os.path.join(str(tmpdir), "exe.py")
    with open(exe, "w") as fp:
        fp.write(
            dedent(
                """\
                import json
                import os
                import pkgutil
                import sys

                import greenlet


                def locate_data():
                    return {
                        "python": "python{major}.{minor}".format(
                            major=sys.version_info[0], minor=sys.version_info[1]
                        ),
                        "site-packages": os.path.dirname(os.path.dirname(greenlet.__file__)),
                        "sys-prefix": sys.prefix,
                    }


                if __name__ == "__main__":
                    json.dump(locate_data(), sys.stdout)
                """
            )
        )

    primary_pex = os.path.join(str(tmpdir), "primary.pex")
    subprocess.check_call(
        args=[
            "pex",
            "--exe",
            exe,
            "-o",
            primary_pex,
            "--runtime-pex-root",
            pex_root,
        ]
    )

    injected_pex = compare(
        primary_pex,
        args=["--version"],
        env=dict(PEX_PATH=data_dirs_pex, PEX_SCRIPT="uwsgi", PEXRC_ROOT=pexrc_root),
    )

    data = json.loads(
        subprocess.check_output(
            args=[injected_pex], env=dict(os.environ, PEX_PATH=data_dirs_pex, PEXRC_ROOT=pexrc_root)
        )
    )
    assert os.path.exists(os.path.join(data["site-packages"], "uwsgidecorators.py")), (
        "Expected .data/purelib to be spread to site-packages."
    )
    assert os.path.exists(
        os.path.join(
            data["sys-prefix"], "include", "site", data["python"], "greenlet", "greenlet.h"
        )
    ), "Expected .data/headers to be spread to Pip's include/site/... dir."
