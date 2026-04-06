# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0
import atexit
import os.path
import shutil
import subprocess
import sys
import sysconfig
import tempfile

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


def seed_pexrc_root(
    session_dir,  # type: str
    pexrc,  # type: str
):
    # type: (...) -> str

    pexrc_root = os.path.join(session_dir, "pexrc-root")
    pex = os.path.join(pexrc_root, "seed.pex")
    subprocess.check_call(args=["pex", "cowsay==5", "-c", "cowsay", "-o", pex])
    subprocess.check_call(args=[pexrc, "inject", pex])
    subprocess.check_call(
        args=[sys.executable, pex + "rc", "Seeded!"], env=dict(PEXRC_ROOT=pexrc_root)
    )
    return pexrc_root


def run_tests():
    # type: () -> Any

    pexrc = ensure_pexrc()
    env = os.environ.copy()
    session_dir = tempfile.mkdtemp(prefix="pexrc-pytest.", suffix=".session")
    atexit.register(shutil.rmtree, session_dir)
    env.update(
        _PEXRC_TEST_PEXRC_BINARY=pexrc,
        _PEXRC_TEST_SESSION_DIR=session_dir,
        _PEXRC_TEST_SESSION_PEXRC_ROOT=seed_pexrc_root(session_dir, pexrc),
        PYTHONPATH=os.path.abspath(os.path.join("python")),
    )
    return subprocess.call(
        args=["pytest", "-n", "auto"] + sys.argv[1:],
        cwd=os.path.abspath(os.path.join("python", "tests")),
        env=env,
    )


if __name__ == "__main__":
    sys.exit(run_tests())
