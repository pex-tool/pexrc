# Release Notes

## 0.9.1

This release fixes injected PEXes to properly resolve from legacy PEXes on the PEX_PATH at runtime
when those legacy PEXes expose items from the wheel .data/ dir in wheel chroot stashes.

## 0.9.0

This release adds support for auto-scoping the clibs and python-proxies injected into PEXes when
the PEX contains native wheels. For pure-Python PEXes, you still need to pare down manually using
`--target`.

## 0.8.0

This release adds support for "un-spreading" legacy PEX wheel chroots when injecting a PEXrc and
also for proper spreading of injected wheels at runtime. This covers all content delivered via
wheel .data/ dirs that was previously not handled by `pexrc`.

## 0.7.1

This release fixes injected `--sh-boot` PEXes to honor `PEX_TOOLS=1` and be robust to underlying
venv breaks due to system Python upgrades or uninstalls.

## 0.7.0

This release adds support for PEX_TOOLS when `pexrc` is built with the `tools` feature; e.g.:
```console
PEXRC_CLIB_FEATURES=tools cargo build ...
```

Releases now ship with this feature enabled.

## 0.6.0

This release adds support for installing venv console scripts.

## 0.5.0

This release wires PEX_VERBOSE to logging levels for both the `pexrc` tool and the runtime of the
injected PEXes it creates.

## 0.4.1

This release fixes user code support for `--no-pre-install-wheels` injected PEXes.

## 0.4.0

This release adds support for injecting `--no-pre-install-wheels` PEXes of all layout types.

## 0.3.1

This release fixes user code support for `--layout {loose,packed}` injected PEXes.

## 0.3.0

Add support for injecting `--layout loose` PEXes.

## 0.2.0

Add support for injecting `--layout packed` PEXes.

## 0.1.0 

Initial release.

