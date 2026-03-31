# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess
import zipfile
from contextlib import closing

import pytest
from testing.compare import compare
from testing.zipfile import ZIP_ZSTANDARD

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, List, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


@pytest.mark.parametrize(
    "layout_args",
    [
        pytest.param([], id="zipapp"),
        pytest.param(["--layout", "packed"], id="packed"),
        pytest.param(["--layout", "loose"], id="loose"),
    ],
)
def test_no_pre_install_wheels(
    tmpdir,  # type: Any
    layout_args,  # type: List[str]
):
    # type: (...) -> None

    pex_root = os.path.join(str(tmpdir), "pex-root")
    pex = os.path.join(str(tmpdir), "cowsay.pex")
    subprocess.check_call(
        args=[
            "pex",
            "cowsay<6",
            "-c",
            "cowsay",
            "--no-pre-install-wheels",
            "--runtime-pex-root",
            pex_root,
            "-o",
            pex,
        ]
        + layout_args
    )

    def test_result(
        result,  # type: ProcessResult
        _is_traditional_pex,  # type: bool
    ):
        # type: (...) -> None
        if _is_traditional_pex:
            return
        result.assert_success()
        assert "| Moo! |" in result.stdout

    injected_pex = compare(
        pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=test_result,
    )
    if zipfile.is_zipfile(injected_pex):
        with closing(zipfile.ZipFile(injected_pex)) as zip_fp:
            cowsay_files = tuple(
                info for info in zip_fp.infolist() if info.filename.startswith(".deps/cowsay")
            )

        assert len(cowsay_files) == 1
        cowsay_whl = cowsay_files[0]
        assert ".deps/cowsay-5.0-py2.py3-none-any.whl" == cowsay_whl.filename
        assert ZIP_ZSTANDARD == cowsay_whl.compress_type, (
            "Expected the whl to be re-compressed using zstd during pexrc injection."
        )
