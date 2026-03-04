# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import os.path
import platform
import subprocess
import sys
import time

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


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

    start = time.time()

    try:
        subprocess.check_call(args=[sys.executable, pex, "Moo!"])
    except subprocess.CalledProcessError as e:
        if platform.system() != "Windows":
            raise e
        # TODO: XXX: Get rid of this once Pex fixes cross-drive commonpath issues.
        print("Expected failure from Pex PEX on Windows: {err}".format(err=e))

    print(
        "Traditional PEX run took {elapsed:.5}ms".format(elapsed=(time.time() - start) * 1000),
        file=sys.stderr,
    )

    subprocess.check_call(args=[pexrc, "inject", pex])

    pexrc_root = os.path.join(str(tmpdir), "pexrc-root")
    env = os.environ.copy()
    env.update(PEXRC_ROOT=pexrc_root)

    start = time.time()
    subprocess.check_call(args=[sys.executable, pex + "rc", "Moo!"], env=env)
    print(
        "pexrc.boot import and run took {elapsed:.5}ms".format(
            elapsed=(time.time() - start) * 1000
        ),
        file=sys.stderr,
    )
