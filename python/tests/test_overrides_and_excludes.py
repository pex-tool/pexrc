# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import contextlib
import glob
import json
import os
import subprocess
import sys
import zipfile
from textwrap import dedent

import pytest
from testing import IS_CI, IS_WINDOWS, IS_X86_64
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


@pytest.mark.skipif(
    IS_CI and IS_WINDOWS and IS_X86_64,
    reason="The example PEX fails to build on Windows x86_64 CI for unknown reasons (works in a VM)",
)
def test_overrides_and_excludes(
    tmpdir,  # type: Any
    pexrc_root,  # type: str
):
    # type: (...) -> None

    project = os.path.join(str(tmpdir), "project")
    os.mkdir(project)
    with open(os.path.join(project, "setup.py"), "w") as fp:
        fp.write(
            dedent(
                """\
                from setuptools import setup


                setup()
                """
            )
        )
    with open(os.path.join(project, "setup.cfg"), "w") as fp:
        fp.write(
            dedent(
                """\
                [metadata]
                name = example
                version = 0.1.0

                [options]
                install_requires =
                    cowsay<6
                    requests==2.33.1
                    psutil
                """
            )
        )
    with open(os.path.join(project, "pyproject.toml"), "w") as fp:
        fp.write(
            dedent(
                """\
                [build-system]
                requires = ["setuptools"]
                build-backend = "setuptools.build_meta"
                """
            )
        )

    wheels = os.path.join(str(tmpdir), "wheels")
    subprocess.check_call(args=["pyproject-build", "--wheel", "-o", wheels, project])
    built_wheels = glob.glob(os.path.join(wheels, "*.whl"))
    assert len(built_wheels) == 1
    example_wheel = built_wheels[0]

    example_pex = os.path.join(str(tmpdir), "example.pex")
    subprocess.check_call(
        args=[
            "pex",
            example_wheel,
            "--exclude",
            "requests<3",
            "--override",
            "cowsay==6",
            "--override",
            "psutil=ansicolors==1.1.8",
            "-o",
            example_pex,
        ]
    )

    injected_pex = compare(
        example_pex,
        args=[
            "-c",
            dedent(
                """\
                import sys

                try:
                    import requests
                    sys.exit("Imported requests from {file}".format(file=requests.__file__))
                except ImportError:
                    pass

                import colors
                import cowsay


                cowsay.tux(colors.cyan("Moo?"))
                """
            ),
        ],
        env=dict(PEXRC_ROOT=pexrc_root),
    )

    with contextlib.closing(zipfile.ZipFile(injected_pex)) as zf:
        pex_info = json.loads(zf.read("PEX-INFO"))

    assert [
        "ansicolors-1.1.8-py2.py3-none-any.whl",
        "cowsay-6.0-py{major}-none-any.whl".format(major=sys.version_info[0]),
        "example-0.1.0-py{major}-none-any.whl".format(major=sys.version_info[0]),
    ] == sorted(pex_info["distributions"])
