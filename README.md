# Steam Shader Guard

A small Rust tool for **native Steam on Linux with NVIDIA graphics**. It detects
wrapped offsets in NVIDIA shader-cache indexes, recovers intact records into
verified copies, and keeps game-written caches outside Steam's cache merger.

Version **0.1.0 is an experimental local workaround**, not an official Valve or
NVIDIA fix. No FPS improvement is promised. The Steam client issue remains tracked
in [ValveSoftware/steam-for-linux#13622](https://github.com/ValveSoftware/steam-for-linux/issues/13622).

## What it does

- Finds native Steam libraries and installed applications.
- Inspects CDVN `.bin`/`.toc` pairs and identifies offsets wrapped at 4 GiB.
- Recovers uniquely identifiable records into files **smaller than 2 GiB**.
- Verifies copied payloads with SHA-256 and checks every rebuilt index.
- Gives each enabled game its own writable NVIDIA cache and read-only seed files.
- Adds a **Steam (Shader Guard)** menu entry that skips NVIDIA Fossilize replay,
  while leaving Steam shader/video downloads and the driver's cache enabled.
- Previews configuration changes; writes only when `--apply` is present.
- Restores its own launch-option changes and retains all shader data on uninstall.

It does not tune kernels, schedulers, graphics quality, frame caps, Proton versions,
or GPU clocks. It does not send reports or make network requests of its own.
Starting Steam naturally starts Steam's normal network activity.

## Requirements and scope

- Linux, NVIDIA's proprietary/open kernel-module driver stack, **native** Steam.
- Flatpak and Snap Steam are **not supported** in this release.
- The supplied executable targets x86-64 Linux and is statically linked with musl.
  Building from source requires Rust 1.85 or newer and Cargo; dependencies are locked.
- Run as your desktop user. Installation and recovery do not require `sudo`.
- Enough free disk space for a separate copy of the cache. Originals stay in place.

The shared program uses its own data directory. It does not automatically import
other tools' private caches or migrate an existing custom gaming setup.

## Quick start

Extract the release archive and open a terminal in that directory. Close Steam
and Wine games before recovering caches or editing launch options.

```sh
./steam-shader-guard doctor
./steam-shader-guard install
./steam-shader-guard install --apply
```

`install` prints its plan first. With `--apply` it installs the program under
`~/.local/bin` and adds **Steam (Shader Guard)** to your application menu.
Your existing Steam shortcuts and shell configuration stay unchanged.
If an installation update fails, fix the reported filesystem error and rerun
`install --apply`; the journal recognizes both sides of an interrupted file update.

For an affected game, replace `2357570` with its Steam app ID:

```sh
./steam-shader-guard scan 2357570
./steam-shader-guard recover 2357570
./steam-shader-guard recover 2357570 --apply
./steam-shader-guard enable 2357570
./steam-shader-guard enable 2357570 --apply
```

Recover an existing warm cache **before the first protected game launch**. Recovery
refuses to overwrite an existing private cache, including one already created by
the game. `recover` without `--apply` only inspects the source and shows its findings.

Now start **Steam (Shader Guard)** and launch the game normally in Steam. This
startup setting is session-wide for NVIDIA replay; cache isolation applies only
to games connected to Shader Guard. Starting the original Steam shortcut, an old
game shortcut, or `/usr/bin/steam` directly can bypass the replay setting.

Games without an existing cache can be enabled directly. Their new shaders still
need to compile on first encounter. Game updates and driver updates can also cause
legitimate recompilation. Skipping Steam replay can move this first-use work into
the game; it does not magically eliminate shader compilation.

## Multiple games and existing launch options

Preview all installed applications, excluding known Steam runtime/Proton tools:

```sh
./steam-shader-guard enable --all
./steam-shader-guard enable --all --apply
```

This enables applications whose launch options are empty. It **does not recover
their existing caches automatically**. Run `recover` first for affected games.
Newly installed games require a later `enable` command.

Custom launch options are preserved and reported as skipped. To integrate manually,
put the installed program immediately around the game's command, retaining your
existing environment variables and wrappers. A plain example is:

```text
'/absolute/path/to/.local/bin/steam-shader-guard' run -- %command%
```

Use the actual absolute path printed by `enable`; do not paste the placeholder.
Use only one `%command%` in the complete line. Explicit custom NVIDIA cache paths
are respected, so a game that already sets its own path is not moved automatically.
Manually added launch options must also be removed manually before uninstalling.

With multiple Steam accounts, select the numeric account directory under Steam's
`userdata` folder:

```sh
./steam-shader-guard enable 2357570 --account 12345678 --apply
```

For nonstandard native Steam installations, use `--steam-root /path/to/Steam`.
For an immutable backup instead of the live cache directory:

```sh
./steam-shader-guard scan 2357570 --source /path/to/backup/nvidiav1
./steam-shader-guard recover 2357570 --source /path/to/backup/nvidiav1 --apply
```

The source must be an NVIDIA cache tree, normally containing `GLCache`, not the
entire Steam library. Shader data is specific to a driver and device; do not
redistribute someone else's cache or copy it between unrelated configurations.

## Undo

Close Steam and Wine games first:

```sh
~/.local/bin/steam-shader-guard disable 2357570 --apply
~/.local/bin/steam-shader-guard uninstall
~/.local/bin/steam-shader-guard uninstall --apply
```

The tool restores tracked launch options only when they still match its changes.
It preserves subsequent edits and refuses to remove the program when a known
modified/manual launch option still references it. Remove manual references first,
including custom launchers outside the Steam accounts known to the tool.
Steam VDF keys are matched case-insensitively; unrelated text is preserved.

Options are command-specific, as shown by `--help`; unsupported or duplicate
options are errors rather than silently ignored. In particular, `uninstall` does
not accept an app ID or account selector. Use `disable APPID --apply` to disconnect
one game; it restores that game's tracked entries across accounts.

Original caches, recovered seed files, new shaders and the small state journal
are retained. It does not delete gigabytes of cache as a side effect of uninstalling.

## Data locations

| Item | Default location |
| --- | --- |
| Installed executable | `~/.local/bin/steam-shader-guard` |
| Protected Steam menu entry | `~/.local/share/applications/steam-shader-guard.desktop` |
| Per-game cache | `~/.local/share/steam-shader-guard/games/<appid>/nvidia/` |
| Seed names and recovery report | Next to each game's `nvidia` directory |
| Reversible settings journal | `~/.local/state/steam-shader-guard/state.json` |

`XDG_DATA_HOME` and `XDG_STATE_HOME` are honored; unset or empty values use the
defaults above. The optional `SHADER_GUARD_HOME`
variable provides an isolated home directory for tests without changing `HOME`.
The wrapper supplies a 12 GB driver cache-size preference unless a game explicitly
sets another value. Read-only seeds use additional disk space; Steam/driver cleanup
settings mean this is **not a hard quota on the total cache directory**.

## Validation and limits

The Rust scanner reproduced **116,283 wrapped offsets in 465,632 records** from a
preserved real-world Overwatch cache across 17 file pairs. These are index records,
not unique shaders or a measurement of compilation throughput.

The original local workaround was exercised on CachyOS, an RTX 5070 Ti and driver
615.71.09. That game opened its recovered cache successfully. This standalone Rust
release has automated recovery and installation tests and a read-only validation
against that real cache; it has **not yet had broad distro/driver testing or a
separate end-to-end gameplay benchmark**. See [VALIDATION.md](VALIDATION.md).

The tool rejects missing pairs, unknown/truncated layouts, ambiguous offsets,
overlaps, unindexed data, symlinks within source trees and existing recovery
destinations. A malformed payload cannot be rebuilt from nothing: recovery only
copies records whose index locations can be identified uniquely. Hash verification
proves that copied bytes match; it does not validate the driver's compressed format.

Source metadata and inventory are checked before and after recovery. Close Steam
as instructed to avoid concurrent writers. Interrupted recovery does not publish a
partial destination; a forced kill can leave a hidden `.shader-guard-*` temporary
directory in the output parent, which can be removed once no recovery is running.

Steam is proprietary. A true upstream repair needs correct handling of the cache
format's 32-bit offsets, bounded files and validation in Steam's merger. This tool
works around that merger and can be removed after an upstream fix has been verified.
Future Steam versions may change the replay control; check `shader_log.txt` for
`Replay currently disabled on NVIDIA.` after starting the protected launcher.

## Build and test

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

For the static x86-64 build:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
```

The tests use temporary directories, including an actual sparse file crossing the
4 GiB boundary. They do not write to the real Steam configuration or launch games.
Integration tests need an ordinary non-root user and no running Steam/Wine processes
visible in the test process namespace. No automated tests require a GPU.

## Sources and license

- [Steam NVIDIA merge issue](https://github.com/ValveSoftware/steam-for-linux/issues/13622)
- [CDVN format notes](https://github.com/therontarigo/nvcachetools/blob/main/format.txt)

The Rust implementation is original and uses the format notes as documentation.
MIT license; see [LICENSE](LICENSE). Bundled dependency notices accompany release
archives. No affiliation with Valve or NVIDIA. The package includes no personal
Steam configuration, shader payloads, account details or recordings.
