# Validation — 0.1.0

Recorded on 2026-10-06. Toolchain: Rust/Cargo 1.99.0. Declared minimum Rust
version: 1.85; the minimum toolchain has not been separately tested.

## Automated checks

- 18 Rust unit tests and 6 CLI integration tests.
- Both unit and integration tests executed as musl-linked Linux executables.
- `cargo clippy --locked --all-targets -- -D warnings`.
- `cargo fmt --check`.
- Release executable: stripped, statically linked x86-64 Linux PIE.
- Release archive includes source, lockfile, tests, documentation and notices;
  no private Steam configuration or shader-cache payloads.

Coverage includes an actual sparse BIN file crossing 4 GiB, copy verification,
bounded shard output, rejected malformed input, failed recovery cleanup,
no-clobber publication, lossless VDF edits, library discovery, preserved game
arguments/exit status, existing custom launch options, previews without writes,
installation/undo, retained cache data and protection against dangling references.

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
