# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

import os.path
import subprocess
import sys
import sysconfig

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


def ensure_pexrc():
    # type: () -> str

    profile = os.environ.get("PEXRC_PROFILE", "dev")
    subprocess.check_call(args=["cargo", "build", "--profile", profile])
    profile_dir = "debug" if profile == "dev" else profile
    return os.path.abspath(
        os.path.join("target", profile_dir, "pexrc" + sysconfig.get_config_vars()["EXE"])
    )


def run_tests():
    # type: () -> Any

    pexrc = ensure_pexrc()
    env = os.environ.copy()
    env.update(_PEXRC_TEST_PEXRC_BINARY=pexrc, PYTHONPATH=os.path.abspath(os.path.join("python")))
    return subprocess.call(
        args=["pytest", "-n", "auto"] + sys.argv[1:],
        cwd=os.path.abspath(os.path.join("python", "tests")),
        env=env,
    )


if __name__ == "__main__":
    sys.exit(run_tests())
