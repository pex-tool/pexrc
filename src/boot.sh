#!/bin/sh

# N.B.: This script should stick to syntax defined for POSIX `sh` and avoid non-builtins.
# See: https://pubs.opengroup.org/onlinepubs/9699919799/idx/shell.html
set -eu

# --- vars --- #
# N.B.: These vars are templated in by pexrc when it injects a PEX with its runtime.
RAW_DEFAULT_PEXRC_ROOT="{pexrc_root}"
VENV_RELPATH="{venv_relpath}"
PYTHONS="{pythons}"
# --- vars --- #

# N.B.: The SC2116 warning suppressions below are in place to ensure tilde-expansion of the
# DEFAULT_PEX_ROOT value which is necessary for the -x check of the venv pex to succeed when it
# should.
if [ -n "${RAW_DEFAULT_PEXRC_ROOT}" ]; then
    # shellcheck disable=SC2116
    DEFAULT_PEXRC_ROOT="$(echo ${RAW_DEFAULT_PEXRC_ROOT})"
else
    if uname -s | grep -iE 'mac|darwin' > /dev/null; then
        # shellcheck disable=SC2116
        DEFAULT_PEXRC_ROOT="$(echo ~/Library/Caches/pexrc)"
    else
        # shellcheck disable=SC2116
        DEFAULT_PEXRC_ROOT="$(echo ~/.cache/pexrc)"
    fi
fi

PEXRC_ROOT="${PEXRC_ROOT:-${DEFAULT_PEXRC_ROOT}}"
VENV="${PEXRC_ROOT}/${VENV_RELPATH}"

on_fast_path() {
    [ -z "${PEX_IGNORE_RCFILES:-}" ] \
      && [ -z "${PEX_PYTHON:-}" ] \
      && [ -z "${PEX_PYTHON_PATH:-}" ] \
      && [ -z "${PEX_PATH:-}" ]
}

if on_fast_path; then
  for python in ${PYTHONS} ; do
      if [ -x "${VENV}/sh-boot/${python}" ]; then
          # The fast path: We're a installed under the PEXRC_ROOT and the venv interpreter to use is
          # embedded in the shebang of our venv pex script; so just execute that script directly.
          export PEX="$0"

          # TODO: XXX: Instead of linking the venv pex script to the sh-boot python, link the
          # venv python. This will ensure when a base interpreter gets uninstalled, the venv will
          # automatically invalidate. As it stands the venv pex script we link to can exist but have
          # a shebang whose python is gone.
          exec "${VENV}/sh-boot/${python}" "$@"
      fi
  done
fi

find_python() {
    for python in ${PYTHONS} ; do
        if command -v "${python}" 2>/dev/null; then
            return
        fi
    done
}

# The slow path: This PEX zipapp is not installed yet. Run the PEX zipapp so it can install itself,
# rebuilding its fast path layout under the PEXRC_ROOT.
python_exe="$(find_python)"
if [ -n "${python_exe}" ]; then
    if [ -n "${PEX_VERBOSE:-}" ]; then
        echo >&2 "$0 used /bin/sh boot to select python: ${python_exe} for re-exec..."
        echo >&2 "Running pex to lay itself out under PEXRC_ROOT."
    fi
    # TODO: XXX: Inject -sE or -I if hermetic?
    export _PEXRC_SH_BOOT_SEED_DIR="${VENV}/sh-boot"
    exec "${python_exe}" "$0" "$@"
fi

echo >&2 "Failed to find any of these python binaries on the PATH:"
for python in ${PYTHONS} ; do
    echo >&2 "${python}"
done
echo >&2 "Either adjust your \$PATH which is currently:"
echo >&2 "${PATH}"
echo >&2 "Or else install an appropriate Python that provides one of the binaries in this list."
exit 1