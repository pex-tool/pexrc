# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

import os.path
import subprocess
import sys

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


def ensure_clib():
    # type: () -> None

    env = os.environ.copy()
    env.update(PEXRC_LIB_DIR=os.path.abspath(os.path.join("python", "pexrc", "__pex__", ".lib")))
    subprocess.check_call(args=["cargo", "build", "--release"], env=env)


def run_tests():
    # type: () -> Any

    ensure_clib()
    return subprocess.call(
        args=["pytest"] + sys.argv[1:], cwd=os.path.abspath(os.path.join("python", "tests"))
    )


if __name__ == "__main__":
    sys.exit(run_tests())
