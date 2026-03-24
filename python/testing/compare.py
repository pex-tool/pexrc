# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

from __future__ import absolute_import, print_function

import os
import subprocess
import sys
import time

from testing import pexrc_inject

TYPE_CHECKING = False
if TYPE_CHECKING:
    # Ruff doesn't understand Python 2 and thus the type comment usages.
    from typing import Any, Callable, Iterable, Mapping, Optional, Text  # noqa: F401


class ProcessResult(object):
    def __init__(
        self,
        exit_code,  # type: int
        stdout,  # type: Text
        stderr,  # type: Text
        elapsed,  # type: float
    ):
        self.exit_code = exit_code
        self.stdout = stdout
        self.stderr = stderr
        self.elapsed = elapsed

    def assert_success(self):
        assert self.exit_code == 0, "Process exited with {exit_code} and STDERR:\n{stderr}".format(
            exit_code=self.exit_code, stderr=self.stderr
        )

    def assert_failure(self):
        assert self.exit_code != 0


def execute_pex(
    pex,  # type: str
    python_args=(),  # type: Iterable[str]
    args=(),  # type: Iterable[str]
    **env,  # type: str
):
    # type" (...) -> ProcessResult

    cmd = [sys.executable]
    cmd.extend(python_args)
    cmd.append(pex)
    cmd.extend(args)

    start = time.time()
    process = subprocess.Popen(
        args=cmd,
        env=dict(os.environ, **env),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    stdout, stderr = process.communicate()
    elapsed = time.time() - start
    return ProcessResult(
        exit_code=process.returncode,
        stdout=stdout.decode("utf-8"),
        stderr=stderr.decode("utf-8"),
        elapsed=elapsed,
    )


def _test_result(
    result,  # type: ProcessResult
    is_traditional_pex,  # type: bool
    test_result=None,  # type: Optional[Callable[[ProcessResult, bool], None]]
):
    # type: (...) -> None

    if test_result:
        test_result(result, is_traditional_pex)
    else:
        result.assert_success()


def _compare_results(
    traditional_result,  # type: ProcessResult
    injected_result,  # type: ProcessResult
    compare_results=None,  # type: Optional[Callable[[ProcessResult, ProcessResult], None]]
):
    # type: (...) -> None

    if compare_results:
        compare_results(traditional_result, injected_result)
    elif traditional_result.exit_code == 0:
        assert traditional_result.stdout == injected_result.stdout
    else:
        assert traditional_result.stderr == injected_result.stderr


def compare(
    pex,  # type: str
    python_args=(),  # type: Iterable[str]
    args=(),  # type: Iterable[str]
    env=None,  # type: Optional[Mapping[str, str]]
    test_result=None,  # type: Optional[Callable[[ProcessResult, bool], None]]
    compare_results=None,  # type: Optional[Callable[[ProcessResult, ProcessResult], None]]
):
    # type: (...) -> str

    traditional_result = execute_pex(pex, python_args, args, **(env or {}))
    _test_result(traditional_result, True, test_result=test_result)
    print(
        "Traditional PEX run took {elapsed:.5}ms".format(elapsed=traditional_result.elapsed * 1000),
        file=sys.stderr,
    )

    injected_pex = pexrc_inject(pex)
    injected_result = execute_pex(injected_pex, python_args, args, **(env or {}))
    _test_result(injected_result, False, test_result=test_result)
    print(
        "Injected PEXRC run took {elapsed:.5}ms".format(elapsed=injected_result.elapsed * 1000),
        file=sys.stderr,
    )

    assert injected_result.elapsed < traditional_result.elapsed, (
        "An injected PEXRC ({injected_elapsed:.5}ms) should always run faster than a traditional "
        "PEX ({traditional_elapsed:.5}ms).".format(
            injected_elapsed=injected_result.elapsed, traditional_elapsed=traditional_result.elapsed
        )
    )
    print(
        "Sped up by a factor of: {speedup_factor:.2}".format(
            speedup_factor=traditional_result.elapsed / injected_result.elapsed
        ),
        file=sys.stderr,
    )
    _compare_results(traditional_result, injected_result, compare_results=compare_results)
    return injected_pex
