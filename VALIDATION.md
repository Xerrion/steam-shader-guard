# Validation — 0.1.0 and unreleased fixes

Recorded on 2026-10-06. Toolchain: Rust/Cargo 1.99.0. Declared minimum Rust
version: 1.85; the minimum toolchain has not been separately tested.

## Automated checks

- Original release: 18 Rust unit tests and 6 CLI integration tests, executed as
  musl-linked Linux executables.
- Unreleased fixes: 22 unit tests and 15 CLI integration tests, executed on the
  host Linux target in an isolated user/PID namespace.
- `cargo clippy --locked --all-targets -- -D warnings`.
- `cargo fmt --check`.
- `cargo build --release --locked` for the unreleased fixes on the host target.
- Original release executable: stripped, statically linked x86-64 Linux PIE.
- Original release archive includes source, lockfile, tests, documentation and notices;
  no private Steam configuration or shader-cache payloads.

Coverage includes an actual sparse BIN file crossing 4 GiB, copy verification,
bounded shard output, rejected malformed input, failed recovery cleanup,
no-clobber publication, lossless VDF edits, library discovery, preserved game
arguments/exit status, existing custom launch options, previews without writes,
installation/undo, retained cache data and protection against dangling references.

## Reproduced defects

The new regression tests were run against the original implementation at
`f51f4ba`, not just the fixed code. Twelve tests failed, demonstrating these eight
defects; all pass after the fixes. An additional test checks retry and uninstall
after a pending file update has already been published.

| Reproduction | Observed original behavior |
| --- | --- |
| Run `uninstall 42 --apply` against a temporary installation | Accepted the ignored app ID and removed the entire installation. |
| Change VDF key casing | Failed account-layout lookup, accepted ambiguous duplicate keys, and removed the binary despite case-variant manual launch references. |
| Deny access to known/default Steam accounts | Removed the executable without checking the inaccessible launch options. Related regressions cover inaccessible menu directories. |
| Replace the menu entry with a symlink to the same contents | Preserved the symlink but removed the executable it still launched. |
| Deny writes while updating an older managed executable, then retry | The first failure replaced the journal's expected hash; retry rejected the unchanged old executable as modified. |
| Supply a Steam cache path with a trailing slash | Left the game using Steam's cache instead of its isolated cache. |
| Recover into a new subdirectory inside the source | Rejected the operation only after creating directories inside the source. |
| Set XDG directory variables to empty strings | Failed instead of using the standard default directories. |

Regression implementations are in [unit tests](src/tests.rs) and
[CLI tests](tests/cli.rs). All filesystem and permission changes use temporary
fixtures; no live Steam account, cache, or process is modified. The process
namespace isolates the CLI's real idle check rather than disabling it.

## Real cache inspection

A read-only run of the Rust scanner against a preserved Overwatch NVIDIA cache:

| Measurement | Result |
| --- | ---: |
| BIN/TOC pairs | 17 |
| Index records | 465,632 |
| Wrapped offsets | 116,283 |
| BIN bytes | 10,168,238,262 |

The wrapped-offset count matches the earlier independent inspection and recovery.
The source was not changed. The release's enable preview also preserved both
existing game-specific launch options on the real machine.

## What has not been established

The original local implementation successfully started Overwatch using recovered
seed data on CachyOS, RTX 5070 Ti and NVIDIA driver 615.71.09. The portable Rust
program was built and tested separately; it was not installed over that working
configuration and has not yet had a separate gameplay/FPS benchmark. Automated
tests do not prove compatibility with every NVIDIA driver, Steam version or distro.

Flatpak, Snap, AMD/Intel driver caches and cross-driver cache migration are outside
this release's supported scope. No claim of fewer necessary shader compilations,
universal FPS gains or an upstream Steam fix is made.
