# Release Process

## Preparation

### Version Bump and Changelog

1. Bump the version in [`Cargo.toml`](Cargo.toml).
2. Run `cargo run -p package` to update [`Cargo.lock`](Cargo.lock) with the new version
   and as a sanity check on the state of the project.
3. Update [`CHANGES.md`](CHANGES.md) with any changes that are likely to be useful to consumers.
4. Open a PR with these changes and land it on https://github.com/pex-tool/pexrc main.

## Release

### Push Release Tag

Sync a local branch with https://github.com/pex-tool/pexrc main and confirm it has the version bump
and changelog update as the tip commit:

```console
:; git log --stat -1 HEAD
commit 4fc958e47826392f9db1f55d227b047e7c701e32 (HEAD -> release/setup)
Author: John Sirois <john.sirois@gmail.com>
Date:   Thu Mar 26 08:01:21 2026 -0700

    Prepare the 0.1.0 release.

 .github/workflows/ci.yml           |   2 +-
 .github/workflows/release.yml      | 140 ++++++++++++++++++++++++++++++++++++++++++++++++++
 CHANGES.md                         |   6 +++
 RELEASE.md                         |  44 ++++++++++++++++
 crates/package/src/main.rs         |   4 +-
 pyproject.toml                     |  16 ++++--
 scripts/generate-release-hashes.py |  79 ++++++++++++++++++++++++++++
 7 files changed, 284 insertions(+), 7 deletions(-)
```

Tag the release as `v<version>` and push the tag to https://github.com/pex-tool/pexrc main:
```console
$ git tag --sign -am 'Release 0.1.0' v0.1.0
$ git push --tags https://github.com/pex-tool/pexrc HEAD:main
```

The release is automated and will create a GitHub Release page at
[https://github.com/pex-tool/pexrc/releases/tag/v&lt;version&gt;](
https://github.com/pex-tool/pexrc/releases) with binaries for Linux, Mac and Windows.

