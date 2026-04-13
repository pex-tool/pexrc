# Copyright 2026 Pex project contributors.
# SPDX-License-Identifier: Apache-2.0

import atexit
import os
import platform
import shutil
import sys
import tempfile
import uuid
import venv
from pathlib import Path
from typing import Any

IS_WINDOWS = platform.system().lower() == "windows"


def main() -> Any:
    if len(sys.argv) != 2:
        return f"Usage: {sys.argv[0]} [DEST DIR]"

    dest_dir = Path(sys.argv[1])

    venv_dir = Path(tempfile.mkdtemp(prefix="pex.rc.", suffix=".venv-activation-scripts"))
    atexit.register(shutil.rmtree, venv_dir, ignore_errors=True)
    scripts_dir = venv_dir / ("Scripts" if IS_WINDOWS else "bin")
    prompt = uuid.uuid4().hex

    env_builder = venv.EnvBuilder(prompt=prompt)
    context = env_builder.ensure_directories(str(venv_dir))
    env_builder.setup_scripts(context)

    os.makedirs(dest_dir, exist_ok=True)
    for script in scripts_dir.iterdir():
        if not script.is_file():
            continue
        (dest_dir / script.name).write_text(
            script.read_text()
            .replace(prompt, "__PEXRC_VENV_PROMPT__")
            .replace(str(venv_dir), "__PEXRC_VENV_DIR__")
        )

    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        sys.exit(str(e))
