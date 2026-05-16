# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import json
import os
import shutil
import subprocess
import sys
from textwrap import dedent

import pytest
from testing import pexrc_inject

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Iterable  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def create_wheel(
    projects_dir,  # type: str
    wheel_dir,  # type: str
    project_name,  # type: str
    dependencies=(),  # type: Iterable[str]
):
    # type: (...) -> subprocess.Popen

    project_dir = os.path.join(projects_dir, project_name)
    os.makedirs(project_dir)

    with open(os.path.join(project_dir, "setup.py"), "w") as fp:
        print("from setuptools import setup; setup()", file=fp)

    with open(os.path.join(project_dir, "setup.cfg"), "w") as fp:
        fp.write(
            dedent(
                """\
                [metadata]
                name = {name}
                version = 0.1.0

                [options]
                {install_requires}
                """
            ).format(
                name=project_name,
                install_requires="install_requires =\n  {deps}".format(
                    deps="\n  ".join(dependencies)
                )
                if dependencies
                else "",
            )
        )

    with open(os.path.join(project_dir, "pyproject.toml"), "w") as fp:
        fp.write(
            dedent(
                """\
                [build-system]
                requires = ["setuptools"]
                build-backend = "setuptools.build_meta"
                """
            )
        )

    return subprocess.Popen(args=["pyproject-build", "--wheel", "--outdir", wheel_dir, project_dir])


@pytest.fixture
def wheels(tmpdir):
    # type: (Any) -> str

    projects_dir = os.path.join(str(tmpdir), "projects")
    wheel_dir = os.path.join(str(tmpdir), "wheels")
    for process in (
        create_wheel(
            projects_dir,
            wheel_dir,
            "a",
            dependencies=['b; extra == "x"', 'c; extra == "y"', 'd; extra == "z"'],
        ),
        create_wheel(projects_dir, wheel_dir, "b"),
        create_wheel(projects_dir, wheel_dir, "c"),
        create_wheel(projects_dir, wheel_dir, "d"),
        create_wheel(projects_dir, wheel_dir, "f", dependencies=["g"]),
        create_wheel(projects_dir, wheel_dir, "g", dependencies=["h[myextra]"]),
        create_wheel(projects_dir, wheel_dir, "h", dependencies=['i; extra == "myextra"']),
        create_wheel(projects_dir, wheel_dir, "i"),
        create_wheel(projects_dir, wheel_dir, "j", dependencies=["h"]),
    ):
        assert 0 == process.wait()
    return wheel_dir


def test_top_level_differing_extras(
    tmpdir,  # type: Any
    wheels,  # type: str
    pexrc_root,  # type: str
):
    # type: (...) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    pex = os.path.join(str(tmpdir), "pex")

    def assert_expected_resolve(*requirements):
        # type: (*str) -> None
        if os.path.exists(pex_root):
            shutil.rmtree(pex_root)
        subprocess.check_call(
            args=[
                "pex",
                "--pex-root",
                pex_root,
                "--runtime-pex-root",
                pex_root,
                "--include-tools",
                "--pre-resolved-dists",
                wheels,
                "-o",
                pex,
            ]
            + list(requirements)
        )

        def resolve(pex_file):
            return set(
                json.loads(line)["project_name"]
                for line in subprocess.check_output(
                    args=[sys.executable, pex_file, "repository", "info", "-v"],
                    env=dict(os.environ, PEX_TOOLS="1", PEXRC_ROOT=pexrc_root),
                ).splitlines()
            )

        assert {"a", "b", "d"} == resolve(pex)
        assert {"a", "b", "d"} == resolve(pexrc_inject(pex))

    assert_expected_resolve("a>=0.1.0", "a[x,z]")
    assert_expected_resolve("a[x,z]", "a>=0.1.0")


def test_transitive_differing_extras(
    tmpdir,  # type: Any
    wheels,  # type: str
    pexrc_root,  # type: str
):
    # type: (...) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    pex = os.path.join(str(tmpdir), "pex")

    def assert_expected_resolve(*requirements):
        # type: (*str) -> None
        if os.path.exists(pex_root):
            shutil.rmtree(pex_root)
        subprocess.check_call(
            args=[
                "pex",
                "--pex-root",
                pex_root,
                "--runtime-pex-root",
                pex_root,
                "--include-tools",
                "--pre-resolved-dists",
                wheels,
                "-o",
                pex,
            ]
            + list(requirements)
        )

        def resolve(pex_file):
            return set(
                json.loads(line)["project_name"]
                for line in subprocess.check_output(
                    args=[sys.executable, pex_file, "repository", "info", "-v"],
                    env=dict(os.environ, PEX_TOOLS="1", PEXRC_ROOT=pexrc_root),
                ).splitlines()
            )

        assert {
            "f",
            "g",
            "h",
            "i",
            "j",
        } == resolve(pex)
        assert {
            "f",
            "g",
            "h",
            "i",
            "j",
        } == resolve(pexrc_inject(pex))

    assert_expected_resolve("f", "j")
    assert_expected_resolve("j", "f")
