# Validation

Original 0.1.0 and unreleased-fix checks recorded on 2026-10-06. Toolchain:
Rust/Cargo 1.99.0. Declared minimum Rust version: 1.85; the minimum toolchain
has not been separately tested. Those checks are historical and were not rerun
for this documentation update.

External field report added on 2026-11-01, covering observations published on
2026-10-23 and the diagnostic follow-up on 2026-10-24.

## Automated checks

- Original release: 18 Rust unit tests and 6 CLI integration tests, executed as
  musl-linked Linux executables.
- Unreleased fixes: 22 unit tests and 15 CLI integration tests, executed on the
  host Linux target in an isolated user/PID namespace.
- `cargo clippy --locked --all-targets -- -D warnings`.
- `cargo fmt --check`.
- `cargo build --release --locked` for the unreleased fixes on the host target.
- Original release executable: stripped, statically linked x86-64 Linux PIE.
- The initial archive included project source. Corrected release packages contain
  the executable, user documentation, and license notices only. A regression test
  rejects source, manifests, tests, workflows, and build tooling in the archive.

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

## External field report: manual isolated-cache experiment

On 2026-10-23, [yotamleo reported an independent Overwatch 2 experiment in
issue #7](https://github.com/Xerrion/steam-shader-guard/issues/7), followed by
[published diagnostic scripts on 2026-10-24](https://github.com/Xerrion/steam-shader-guard/issues/7#issuecomment-6384643050).

**This was a manual test of cache recovery and isolation, not a test of the
Steam Shader Guard executable.** The reporter used a custom Python splitter
based on `src/cache.rs`: resolve offsets at stored offset + k * 2^32, check
contiguity, keep shards below 2^31 bytes and preserve the 32-byte headers.
The measurements below are attributed to the reporter; they have not been
independently reproduced by this project.

### Test conditions

| Setting | Reported value |
| --- | --- |
| Game | Overwatch 2 (Steam app ID 2357570) |
| Distribution | CachyOS |
| GPU | NVIDIA RTX 4090 |
| Driver | nvidia-open 615.78.08 |
| Proton | proton-cachyos 11.0-20261015 |
| Writable cache | Private directory selected with `__GL_SHADER_DISK_CACHE_PATH` |
| Cache application name | `__GL_SHADER_DISK_CACHE_APP_NAME=overwatch` |
| Requested cache size | `__GL_SHADER_DISK_CACHE_SIZE=17179869184` (16 GiB) |
| Cleanup setting | `__GL_SHADER_DISK_CACHE_SKIP_CLEANUP=1` |
| Recovered seed | Three shards in `__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME`, with exact names separated by `;`; the reporter observed all three being opened |

These are the reporter's settings, not a record of Shader Guard's defaults.

### Reported observations

| Measurement | First launch | Relaunch with the isolated cache retained |
| --- | ---: | ---: |
| Game's own cache written in first 3 minutes | 932 MB | 0 bytes |
| Game's own cache written in first 10 minutes | 2.46 GB | 43 MB |
| Game's own cache written over a whole match | Not reported | 65 MB |

Units and timing above follow the issue report. The relaunch still had about
four minutes of `dxvk-shader` thread activity at approximately 950% aggregate
CPU. The low cache growth supports retention of the game's own cache in this
setup; it does not establish that compilation or pipeline work stopped.
In particular, `warmcheck.sh` samples writable BIN file sizes and thread CPU,
not driver cache hits or compiler invocations.

The reporter also recovered a Steam merged cache containing 262,547 entries,
including 50,673 with wrapped offsets, into three verified shards. In the first
launch, 50,164 of the 108,103 keys in shard 0 appeared again in the game's newly
written cache, reported as recompilation. No such overlap was reported for
shards 1 and 2. Absence of overlap does not prove that those other entries were
used.

For 2,000 of 2,000 sampled pairs sharing a 16-byte key, the recovered record and
the game's fresh record differed in header bytes `[20:28]`, payload size and
payload content. The follow-up comment separately reported the same kinds of
differences for all 500 sampled shared keys. No duplicate key within an
individual file was reported.

These observations indicate that restoring valid offsets and copying records
faithfully does not guarantee driver reuse of every entry. They do not establish
what the differing header fields mean or the driver's exact acceptance rules.
The proposed explanation involving `fossilize_replay` and game-specific driver
application profiles remains an unverified hypothesis. The attribution of the
remaining launch burst to DXVK-side work is also an interpretation, not a
measurement of its exact cause.

### Diagnostic follow-up and limits

The reporter's MIT-licensed
[nvidia-steam-shader-cache toolkit](https://github.com/yotamleo/nvidia-steam-shader-cache)
provides complementary diagnostics. The
[reviewed source snapshot](https://github.com/yotamleo/nvidia-steam-shader-cache/tree/8e7b20d43eacd144c9f6196e7f9fdc1011f35f20)
includes:

- `tools/tocwrap.py`: compare keys at stored offsets and stored offsets + 2^32.
- `tools/split.py`: recover and verify shards below 2 GiB.
- `tools/keyoverlap.py` and `tools/reccmp.py`: compare key sets and records.
- `tools/warmcheck.sh`: sample cache growth and shader-thread CPU.
- `tools/probe-read-only.py`: run `vulkaninfo --summary` and observe file-open
  events for read-only cache names. The toolkit README identifies this separator
  probe as tested on driver 615.71.09, separately from the 615.78.08 gameplay
  report. It reports that `;` opens multiple exact names while a comma list is
  ignored. Opening a file does not prove that its records were reused.

The toolkit and documentation were reviewed for this update; its live probes
were not run here and its code was not incorporated into Shader Guard.
This single reported configuration supports the manual isolation approach and
documents a limitation of recovered seed data. It does not validate Shader
Guard's installation, recovery, launch-option editing or Steam shortcut.
It provides no controlled FPS/frame-time benchmark, general performance gain,
proof of complete shader-compilation elimination, or controlled comparison of
isolation with versus without recovered seed data.

## What has not been established

The original local implementation successfully started Overwatch using recovered
seed data on CachyOS, RTX 5070 Ti and NVIDIA driver 615.71.09. The portable Rust
program was built and tested separately; it was not installed over that working
configuration and has not yet had a separate gameplay/FPS benchmark. Automated
tests do not prove compatibility with every NVIDIA driver, Steam version or distro.

Flatpak, Snap, AMD/Intel driver caches and cross-driver cache migration are outside
this release's supported scope. No claim of fewer necessary shader compilations,
universal FPS gains or an upstream Steam fix is made.
