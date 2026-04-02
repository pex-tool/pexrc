# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os
import subprocess
from textwrap import dedent

import pytest
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, List, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def create_cowsay_pex(
    tmpdir,  # type: Any
    *pex_args,  # type: str
):
    src = os.path.join(str(tmpdir), "src")
    package = os.path.join(src, "package")
    os.makedirs(package)
    with open(os.path.join(package, "module.py"), "w") as fp:
        fp.write(
            dedent(
                """\
                import pkgutil
                import sys

                import cowsay

                def main():
                    cowsay.tux(pkgutil.get_data(__name__, "resources/message").decode("utf-8"))


                if __name__ == "__main__":
                    sys.exit(main())
                """
            )
        )

    resources = os.path.join(package, "resources")
    os.makedirs(resources)
    open(os.path.join(resources, "__init__.py"), "w").close()
    with open(os.path.join(resources, "message"), "w") as fp:
        fp.write("Moo?")

    pex = os.path.join(str(tmpdir), "cowsay.pex")
    pex_root = os.path.join(str(tmpdir), "pex-root")
    subprocess.check_call(
        args=[
            "pex",
            "cowsay<6",
            "-D",
            src,
            "-m",
            "package.module",
            "-o",
            pex,
            "--runtime-pex-root",
            pex_root,
        ]
        + list(pex_args)
    )
    return pex


def assert_result(
    result,  # type: ProcessResult
    _is_traditional_pex,  # type: bool
):
    # type: (...) -> None
    result.assert_success()
    assert "| Moo? |" in result.stdout


@pytest.mark.parametrize(
    "layout_args",
    [
        pytest.param([], id="zipapp"),
        pytest.param(["--layout", "packed"], id="packed"),
        pytest.param(["--layout", "loose"], id="loose"),
    ],
)
@pytest.mark.parametrize(
    "whl_args",
    [
        pytest.param([], id="chroot"),
        pytest.param(["--no-pre-install-wheels"], id="whl"),
    ],
)
def test_user_sources(
    tmpdir,  # type: Any
    layout_args,  # type: List[str]
    whl_args,  # type: List[str]
):
    # type: (...) -> None

    compare(
        create_cowsay_pex(tmpdir, *(layout_args + whl_args)),
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=assert_result,
    )
