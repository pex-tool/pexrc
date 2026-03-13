# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import pytest
import testing


@pytest.fixture
def pexrc():
    # type: () -> str
    return testing.pexrc()
