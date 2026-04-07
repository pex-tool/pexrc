# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess
from textwrap import dedent

from testing import IS_WINDOWS, pexrc

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


def test_create_script(tmpdir):
    # type: (Any) -> None

    venv = os.path.join(str(tmpdir), "venv")
    subprocess.check_call(args=["uv", "venv", "--no-project", venv])
    subprocess.check_call(args=["uv", "pip", "install", "--python", venv, "cowsay==5"])
    python = subprocess.check_output(args=["uv", "python", "find", venv]).decode("utf-8").strip()
    script = os.path.join(str(tmpdir), "cowsay.py")
    with open(script, "w") as fp:
        fp.write(
            dedent(
                """\
                import sys

                import cowsay


                def tux(msg):
                    cowsay.tux(msg)


                if __name__ == "__main__":
                    tux("Linus says: " + " ".join(sys.argv[1:]))
                """
            )
        )

    if IS_WINDOWS:
        script_exe = os.path.join(venv, "Scripts", "cowsay.exe")
    else:
        script_exe = os.path.join(venv, "bin", "cowsay")
    assert os.path.isfile(script_exe), "The venv setup above should have installed a cowsay script."
    assert b"| Moo? |" in subprocess.check_output(args=[script_exe, "Moo?"])

    os.unlink(script_exe)

    subprocess.check_call(args=[pexrc(), "script", "--python", python, "-o", script_exe, script])

    assert b"| Linus says: Moo? |" in subprocess.check_output(args=[script_exe, "Moo?"])
