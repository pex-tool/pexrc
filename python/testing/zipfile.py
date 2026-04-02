# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import

# The method ID of the zstd compression type in injected PEX zips. This was determined by
# observation but also matches newer Python support.
#
# N.B.: We can't rely on the zipfile.ZIP_ZSTANDARD constant being available since its Python 3.14+
# only. See: https://docs.python.org/3/library/zipfile.html#zipfile.ZIP_ZSTANDARD
ZIP_ZSTANDARD = 93
