# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

import os

import pytest


@pytest.fixture
def pexrc():
    # type: () -> str
    return os.environ["_PEXRC_TEST_PEXRC_BINARY"]
