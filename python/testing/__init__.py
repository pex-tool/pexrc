# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os
import platform
import subprocess

IS_WINDOWS = platform.system() == "Windows"


def pexrc():
    # type: () -> str
    return os.environ["_PEXRC_TEST_PEXRC_BINARY"]


def pexrc_inject(pex):
    # type: (str) -> str

    subprocess.check_call(args=[pexrc(), "inject", pex])
    injected_pex = pex + "rc"
    assert os.path.isfile(injected_pex)
    return injected_pex
