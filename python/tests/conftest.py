# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os.path

import pytest
from testing import IS_MAC, session_pexrc_root

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any  # noqa: F401


@pytest.fixture
def pexrc_root(tmpdir):
    # type: (Any) -> str

    pexrc_root = os.path.join(str(tmpdir), "pexrc-root")
    if IS_MAC:
        # N.B.: Mac SIP (System Integrity Protection) is an umbrella under which new binaries are
        # scanned for provenance. This scanning is slow to the point initial startup for the 1st
        # PEXrc to use a given Python on the system becomes almost as slow as a traditional PEX;
        # sometimes coming out slower. Although we have code to only suffer this hit once per system
        # interpreter across any PEXrcs run on the machine, tests start with fresh pexrc-roots and
        # all suffer this hit. To better simulate the Mac case, we pre-seed just this portion of
        # the cache so test comparisons can look past this amortized 1-time slow start.
        src = os.path.join(session_pexrc_root(), "python-proxies")
        dst = os.path.join(pexrc_root, "python-proxies")
        for root, dirs, files in os.walk(src):
            rel_root = os.path.relpath(root, src) if root != src else ""
            for d in dirs:
                os.makedirs(os.path.join(dst, rel_root, d))
            for f in files:
                os.symlink(os.path.join(root, f), os.path.join(dst, rel_root, f))
    return pexrc_root
