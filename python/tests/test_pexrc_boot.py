# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import os.path
import platform
import subprocess
import sys
import time
from textwrap import dedent

import colors  # type: ignore[import-untyped]

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Text  # noqa: F401


def run_traditional_pex(
    pex,  # type: str
    *args,  # type: str
    **env,  # type: str
):
    # type: (...) -> Text

    start = time.time()

    try:
        output = subprocess.check_output(
            args=[sys.executable, pex] + list(args), env=dict(os.environ, **env)
        )
    except subprocess.CalledProcessError as e:
        if platform.system() != "Windows":
            raise e
        # TODO: XXX: Get rid of this once Pex fixes cross-drive commonpath issues.
        print("Expected failure from Pex PEX on Windows: {err}".format(err=e))
        output = b""

    print(
        "Traditional PEX run took {elapsed:.5}ms".format(elapsed=(time.time() - start) * 1000),
        file=sys.stderr,
    )
    return output.decode("utf-8")


def run_injected_pex(
    pexrc,  # type: str
    pex,  # type: str
    *args,  # type: str
    **env,  # type: str
):
    # type: (...) -> Text

    subprocess.check_call(args=[pexrc, "inject", pex])

    start = time.time()
    output = subprocess.check_output(
        args=[sys.executable, pex + "rc"] + list(args), env=dict(os.environ, **env)
    )
    print(
        "pexrc.boot import and run took {elapsed:.5}ms".format(
            elapsed=(time.time() - start) * 1000
        ),
        file=sys.stderr,
    )
    return output.decode("utf-8")


def test_boot(
    tmpdir,  # type: Any
    pexrc,  # type: str
):
    # type: (...) -> None

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

    run_traditional_pex(pex, "Moo!")
    run_injected_pex(pexrc, pex, "Moo!", PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root"))


def test_pex_path(
    tmpdir,  # type: Any
    pexrc,  # type: str
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

    expected_message = "| {message} |".format(message=colors.cyan("Moo?"))
    assert expected_message in run_traditional_pex(cowsay_pex, "Moo?", PEX_PATH=ansicolors_pex)

    assert expected_message == run_injected_pex(
        pexrc,
        cowsay_pex,
        "Moo?",
        PEX_PATH=ansicolors_pex,
        PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root"),
    )
