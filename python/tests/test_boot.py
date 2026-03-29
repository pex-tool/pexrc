# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path
import subprocess

from testing import IS_WINDOWS, pexrc_inject
from testing.compare import compare

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Text  # noqa: F401

    from testing.compare import ProcessResult  # noqa: F401


def create_cowsay_pex(
    tmpdir,  # type: Any
    *pex_args,  # type: str
):
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
        + list(pex_args)
    )
    return pex


def assert_result(
    result,  # type: ProcessResult
    _is_traditional_pex,  # type: bool
):
    # type: (...) -> None
    result.assert_success()
    assert "| Moo! |" in result.stdout


def read_shebang(pex):
    # type: (str) -> Text

    with open(pex, "rb") as fp:
        return fp.readline().decode("utf-8")


def test_basic(tmpdir):
    # type: (Any) -> None

    pex = create_cowsay_pex(tmpdir)
    expected_shebang = read_shebang(pex)
    assert expected_shebang.startswith("#!/usr/bin/env ")

    injected_pex = compare(
        pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=assert_result,
    )
    assert expected_shebang == read_shebang(injected_pex)


def test_sh_boot(tmpdir):
    # type: (Any) -> None

    pex = create_cowsay_pex(tmpdir, "--sh-boot")
    expected_shebang = read_shebang(pex)
    assert expected_shebang == "#!/bin/sh\n"

    pexrc_env = dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root"))
    injected_pex = compare(
        pex,
        args=["Moo!"],
        env=pexrc_env,
        test_result=assert_result,
    )
    assert expected_shebang == read_shebang(injected_pex)

    # N.B.: The above uses compare which executes python against the PEX, which just proves the
    # `--sh-boot` shebang does not interfere with that. As long as we're not on Windows, we can run
    # the `--sh-boot` shebang directly.
    if not IS_WINDOWS:
        assert b"| Moo! |" in subprocess.check_output(args=[pex, "Moo!"])
        assert b"| Moo! |" in subprocess.check_output(args=[injected_pex, "Moo!"], env=pexrc_env)


def test_packed(tmpdir):
    # type: (Any) -> None

    pex = create_cowsay_pex(tmpdir, "--layout", "packed")
    assert os.path.isdir(pex)

    injected_pex = compare(
        pex=pex,
        args=["Moo!"],
        env=dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root")),
        test_result=assert_result,
    )
    assert os.path.isdir(injected_pex)


def test_packed_sh_boot(tmpdir):
    # type: (Any) -> None

    pex = create_cowsay_pex(tmpdir, "--layout", "packed", "--sh-boot")
    assert os.path.isdir(pex)
    pex_script = os.path.join(pex, "pex")
    # N.B.: Pex incorrectly uses the host line ending here, which we tolerate for the purposes of
    # this test. A /bin/sh script, though, should always use \n line endings.
    assert "#!/bin/sh{eol}".format(eol=os.linesep) == read_shebang(pex_script)

    injected_pex = pexrc_inject(pex)
    assert os.path.isdir(injected_pex)
    injected_pex_script = os.path.join(injected_pex, "pex")
    assert "#!/bin/sh\n" == read_shebang(injected_pex_script)

    if not IS_WINDOWS:
        pexrc_env = dict(PEXRC_ROOT=os.path.join(str(tmpdir), "pexrc-root"))

        # N.B.: The `--layout packed --sh-boot` PEXes Pex builds are broken on Windows; so we just
        # run the comparison on unix.
        compare(
            pex, args=["Moo!"], env=pexrc_env, test_result=assert_result, injected_pex=injected_pex
        )

        # N.B.: The above uses `compare` which executes python against the PEX, which just proves
        # the `--sh-boot` shebang does not interfere with that. As long as we're not on Windows, we
        # can run the `--sh-boot` shebang directly.
        assert b"| Moo! |" in subprocess.check_output(args=[pex_script, "Moo!"])
        assert b"| Moo! |" in subprocess.check_output(
            args=[injected_pex_script, "Moo!"], env=pexrc_env
        )
