# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess
import zipfile
from contextlib import closing

import pytest
from testing import IS_WINDOWS
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
    pexrc_root,  # type: str
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

    # N.B.: Neither zipapp nor packed traditional PEXes with deps as whl files work on Windows.
    is_windows_non_loose_pex = IS_WINDOWS and (
        os.path.isfile(pex) or os.path.isfile(os.path.join(pex, ".bootstrap"))
    )

    def test_result(
        result,  # type: ProcessResult
        is_traditional_pex,  # type: bool
    ):
        # type: (...) -> None
        if is_traditional_pex and is_windows_non_loose_pex:
            return
        result.assert_success()
        assert "| Moo! |" in result.stdout

    def compare_results(
        traditional_result,  # type: ProcessResult
        injected_result,  # type: ProcessResult
    ):
        if is_windows_non_loose_pex:
            return
        assert traditional_result.stdout == injected_result.stdout

    injected_pex = compare(
        pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=pexrc_root),
        test_result=test_result,
        compare_results=compare_results,
    )
    if zipfile.is_zipfile(injected_pex):
        with closing(zipfile.ZipFile(injected_pex)) as zip_fp:
            cowsay_files = tuple(
                info for info in zip_fp.infolist() if info.filename.startswith(".deps/cowsay")
            )

            assert len(cowsay_files) == 1
            cowsay_whl = cowsay_files[0]
            assert ".deps/cowsay-5.0-py2.py3-none-any.whl" == cowsay_whl.filename
            assert zipfile.ZIP_STORED == cowsay_whl.compress_type, (
                "Expected the whl to be stored in the outer zip with no compression."
            )

            with zipfile.ZipFile(zip_fp.open(cowsay_whl)) as cowsay_zip_fp:
                for entry in cowsay_zip_fp.infolist():
                    if entry.filename.endswith("/"):
                        assert zipfile.ZIP_STORED == entry.compress_type, (
                            "Expected the re-compressed whl to have stored directory entries."
                        )
                    else:
                        assert ZIP_ZSTANDARD == entry.compress_type, (
                            "Expected the re-compressed whl to have zstd compressed files."
                        )
