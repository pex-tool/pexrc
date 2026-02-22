# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import os.path
import subprocess
import sys
import time

import pexrc

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


def test_boot(tmpdir):
    # type: (Any) -> None

    pex = os.path.join(str(tmpdir), "cowsay.pex")
    pex_root = os.path.join(str(tmpdir), "pex_root")
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

    start = time.time()

    try:
        subprocess.check_call(args=[sys.executable, pex, "Moo!"])
    except subprocess.CalledProcessError as e:
        if pexrc.CURRENT_OS != pexrc.WINDOWS:
            raise e
        # TODO: XXX: Get rid of this once Pex fixes cross-drive commonpath issues.
        print("Expected failure from Pex PEX on Windows: {err}".format(err=e))

    print(
        "Traditional PEX run took {elapsed:.5}ms".format(elapsed=(time.time() - start) * 1000),
        file=sys.stderr,
    )

    python_source_root = os.path.abspath(os.path.join(pexrc.__file__, "..", ".."))

    start = time.time()
    subprocess.check_call(
        args=[
            sys.executable,
            "-c",
            "import sys, pexrc; pexrc.boot(r'{pex}', python_args=[], args=['Moo!'])".format(
                pex=pex
            ),
        ],
        cwd=python_source_root,
    )
    print(
        "pexrc.boot import and run took {elapsed:.5}ms".format(
            elapsed=(time.time() - start) * 1000
        ),
        file=sys.stderr,
    )
