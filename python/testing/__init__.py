# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os
import platform
import subprocess

import pytest

IS_WINDOWS = platform.system() == "Windows"


skip_windows_cant_build_pex_to_inject_yet = pytest.mark.skipif(
    IS_WINDOWS, reason="Pex doesn't work on Windows yet; so we can't build a PEX to inject."
)


def pexrc():
    # type: () -> str
    return os.environ["_PEXRC_TEST_PEXRC_BINARY"]


def pexrc_inject(pex):
    # type: (str) -> str

    subprocess.check_call(args=[pexrc(), "inject", pex])
    injected_pex = pex + "rc" if pex.endswith(".pex") else pex + ".pexrc"
    assert os.path.isfile(injected_pex)
    return injected_pex
