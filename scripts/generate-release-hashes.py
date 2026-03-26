# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

import hashlib
import io
import os.path
import sys
from argparse import ArgumentDefaultsHelpFormatter, ArgumentParser
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator, TextIO


class HashVerifyError(Exception):
    pass


def generate_release_hashes(releases_dir: Path, output: TextIO) -> None:
    print("|file|sha256|size|", file=output)
    print("|----|------|----|", file=output)
    for hash_file in sorted(releases_dir.glob("*.sha256")):
        expected_sha256, file_name = hash_file.read_text().split(" ", maxsplit=1)
        path = releases_dir / file_name.lstrip("*")

        digest = hashlib.sha256()
        with path.open("rb") as fp:
            for chunk in iter(lambda: fp.read(io.DEFAULT_BUFFER_SIZE), b""):
                digest.update(chunk)
        actual_sha256 = digest.hexdigest()
        if actual_sha256 != expected_sha256:
            raise HashVerifyError(
                f"Invalid sha256 hash for {path}:\n"
                f"expected: {expected_sha256}\n"
                f"found:    {actual_sha256}"
            )

        print(f"|{path.name}|{actual_sha256}|{os.path.getsize(path)}|", file=output)


@contextmanager
def output(output_file: Path | None = None) -> Iterator[TextIO]:
    if output_file:
        with output_file.open("w") as fp:
            yield fp
    else:
        yield sys.stdout


def main() -> Any:
    parser = ArgumentParser(
        description="Generate a markdown table or release artifact sizes and hashes",
        formatter_class=ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "-i",
        "--releases-dir",
        dest="releases_dir",
        default="dist",
        type=Path,
        help="The directory containing the releases to hash.",
    )
    parser.add_argument(
        "-o",
        "--output-file",
        dest="output_file",
        default=None,
        type=Path,
        help="A file path to emit the markdown table to. If not specified, defaults to stdout.",
    )
    options = parser.parse_args()
    with output(options.output_file) as fp:
        generate_release_hashes(options.releases_dir, fp)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except HashVerifyError as e:
        sys.exit(str(e))
