# Steam Shader Guard

For Linux players with NVIDIA graphics who keep seeing Steam process shaders again.
This tool gives each game a separate folder for saved shaders. It can also copy
reusable shader data from an existing cache.

A **shader cache** holds compiled graphics programs that a game can reuse.
Shader Guard keeps the game's new cache separate from Steam's shader-file merger.
Its Steam shortcut skips NVIDIA shader pre-processing.

**This is an experimental workaround, not an official Valve or NVIDIA fix.**
It does not promise higher FPS or remove all shader compilation.
New effects, game updates and driver updates can still require compilation.

[Dansk vejledning](README.da.md)

## Before you start

- Use the regular Linux Steam app with NVIDIA graphics. Flatpak and Snap Steam
  are not supported.
- The downloadable program runs on x86-64 Linux. You do not need Rust to use it.
- Run commands as your normal desktop user, **not with `sudo`**.
- Copying existing shaders needs extra disk space. Your original files are kept.

There are two setup actions: **install the tool**, then **set up your games**.
Installation alone does not change any game settings.

## Set up a game

### 1. Install Shader Guard

Open a terminal and run:

```sh
curl -fsSL https://raw.githubusercontent.com/Xerrion/steam-shader-guard/main/scripts/install.sh | sh
```

This downloads the latest stable release, checks the downloaded program against
the release's checksum, then installs it. That check makes sure the download
matches the release before running it. It adds **Steam (Shader Guard)** to your
application menu. It does not set up games, copy shaders or start Steam.

The command downloads and runs [this installation script](https://github.com/Xerrion/steam-shader-guard/blob/main/scripts/install.sh).
You can read it first. If you prefer not to run a downloaded script, use the
[manual download instructions](#manual-download).

Run the same command to install a newer release. Files that you changed yourself
will not be overwritten.

### 2. Find your game's ID

Fully exit Steam and any running games. Steam can keep running after you close its
window, so use Steam's **Exit** action.

Then run:

```sh
~/.local/bin/steam-shader-guard doctor
```

This checks your Steam installation and shows a readable list of games.
Find your game and note the number in the **Game ID** column.
The check does not change anything.

The examples below use `2357570`, the ID for Overwatch.
**Replace that number with your game's ID.**

### 3. Copy existing shaders, if you want to reuse them

**This step is optional.** If the game has saved NVIDIA shaders, you can copy the
reusable data before starting it with Shader Guard:

```sh
~/.local/bin/steam-shader-guard recover 2357570 --apply
```

The command checks the original files, makes a separate copy and checks the copy.
It shows which file it is checking or copying. The originals stay unchanged.
This does not set up the game.

If `doctor` shows **not found** under **Steam shader folder**, skip this step.
Without a copy, the game starts with an empty Shader Guard cache and builds it
as you play.

Copy before the game's **first launch with Shader Guard**.
The tool will not overwrite a Shader Guard folder that already exists.

### 4. Set up the game

```sh
~/.local/bin/steam-shader-guard enable 2357570 --apply
```

This changes that game's Steam launch options so it uses Shader Guard when you
start it. The command reports whether it changed the game or skipped it.
It does not copy shaders or start the game.
If the game already has Shader Guard launch options, it leaves them unchanged.

Games with other launch options are also left unchanged. See
[existing launch options](#existing-launch-options) if your game is skipped.

### 5. Start Steam through the new shortcut

Open **Steam (Shader Guard)** from your application menu. Then launch the game
normally in Steam.

You can also start it from a terminal:

```sh
~/.local/bin/steam-shader-guard steam
```

**Use this shortcut each time you start Steam.** Installation and game setup
stay saved, but the shader pre-processing setting only applies to the Steam
session started this way. Your usual Steam shortcut remains unchanged.

Setting up a game without using the new Steam shortcut does not skip Steam's
shader pre-processing. Using the shortcut without setting up the game does not
give that game its separate shader folder.

## Set up more games

Repeat the copy and setup steps for each game you want to use.
You can also set up all eligible installed games at once:

```sh
~/.local/bin/steam-shader-guard enable --all --apply
```

This skips games with existing launch options and Steam support tools such as
Proton. It does not copy any existing shaders. Copy those first if you want to
reuse them. Games you install later need their own setup.

## What does `--apply` mean?

It means **make this command's changes**, not “turn everything on.”

For `install`, `recover`, `enable`, `disable` and `uninstall`, leaving out
`--apply` shows a plan without making changes. For example:

```sh
~/.local/bin/steam-shader-guard enable 2357570
```

You do not have to run the plan first.
`doctor` and `scan` only check files. `steam` and `run` start programs immediately.

## Check a game's saved shaders

```sh
~/.local/bin/steam-shader-guard scan 2357570
```

This checks the existing NVIDIA shader files without changing them.
It reports whether it found stored entries with broken file positions.
It does not copy files or set up the game.

## Existing launch options

To see a game's launch options, right-click it in your Steam library.
Select **Properties**, then **General**, then **Launch Options**.

Shader Guard will not overwrite that box if it already contains text.
If it contains other launch options, the command prints the Shader Guard command
you can add yourself.
Keep your existing settings. If you are unsure how they fit together, leave the
game unchanged rather than replacing them.

<details>
<summary>Adding Shader Guard to custom launch options</summary>

Put Shader Guard immediately before the game's command.
For example, if you use `gamemoderun %command%`, the result has this form:

```text
gamemoderun '/absolute/path/to/.local/bin/steam-shader-guard' run -- %command%
```

Use the actual absolute path printed by `enable`, not the example path.
Keep only one `%command%` in the complete line.
The `run` command sets the shader folder, then starts the game's normal command.
Steam calls it for you after automatic setup.

If your existing settings already select a custom NVIDIA cache folder, Shader
Guard respects that choice instead of moving the cache.
Launch options you add yourself must also be removed yourself before uninstalling.

</details>

## Stop using Shader Guard

Fully exit Steam and any running games first.

To stop using it for one game while leaving the tool installed:

```sh
~/.local/bin/steam-shader-guard disable 2357570 --apply
```

To undo the tool's saved game changes and remove it and its menu shortcut:

```sh
~/.local/bin/steam-shader-guard uninstall --apply
```

**Neither command deletes your shader files.**
The tool keeps any launch options you changed after setup.
If changed options or a changed shortcut still use Shader Guard, it stops removal
and tells you what needs attention. Remove manual references before trying again.

## Command guide

Use `~/.local/bin/steam-shader-guard --help` for the command list.
Use a command followed by `--help` for its options, such as `enable --help`.

| Command | Purpose |
| --- | --- |
| `doctor` | Check Steam and list your installed games and their IDs. |
| `scan GAME_ID` | Check existing shader files without changing them. |
| `recover GAME_ID --apply` | Make a checked, separate copy of reusable shaders. |
| `install --apply` | Install the tool and the new Steam shortcut. |
| `enable GAME_ID --apply` | Set up a game to use Shader Guard. |
| `steam` | Start Steam through Shader Guard. |
| `disable GAME_ID --apply` | Undo the tool's setup for one game. |
| `uninstall --apply` | Undo saved game changes and remove the tool. |
| `run` | Start a game through Shader Guard. Steam normally calls this for you. |

Replace `GAME_ID` with a number from `doctor`. Do not type `GAME_ID` literally.
Messages explain what the tool is doing, what changed and what to do next.

## Manual download

Download the Linux x86-64 archive and `SHA256SUMS` from the
[latest release](https://github.com/Xerrion/steam-shader-guard/releases/latest).
Put both files in the same folder and open a terminal there.

Check the download:

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

Continue only if the archive reports **OK**. Extract it, open a terminal in the
extracted folder, and check its contents:

```sh
sha256sum --check SHA256SUMS &&
./steam-shader-guard install --apply
```

Stop if a checksum check fails. After installation, continue with
[finding your game's ID](#2-find-your-games-id).

The archive includes the program, guides and license notices. It does not include
source code or build tools. GitHub offers separate source downloads.

## Limits and safety

- This is a workaround for
  [ValveSoftware/steam-for-linux#13622](https://github.com/ValveSoftware/steam-for-linux/issues/13622).
  It does not fix Steam itself.
- It only copies data it can identify safely. Missing or ambiguous shader data
  cannot be recreated from nothing.
- A checksum check confirms that copied bytes match. It does not prove the NVIDIA
  driver can use every stored shader.
- Testing is limited. The original workaround ran on CachyOS with an RTX 5070 Ti
  and driver 615.71.09. This release has not had broad testing across Linux
  distributions and NVIDIA drivers. See [what we tested](https://github.com/Xerrion/steam-shader-guard/blob/main/VALIDATION.md).
- The executable sends no reports and makes no network requests.
  The optional installer downloads release files from GitHub.
  Steam still uses its own network connections.

<details>
<summary>Other accounts, folders and script output</summary>

### Multiple Steam accounts

If the tool asks you to choose an account, use the numeric account folder under
Steam's `userdata` folder:

```sh
~/.local/bin/steam-shader-guard enable 2357570 --account 12345678 --apply
```

Replace `12345678` with your account folder's number.

### Other Steam or shader folders

Use `--steam-root /path/to/Steam` if Steam is installed outside the usual locations.
To check or copy an NVIDIA shader folder from a backup:

```sh
~/.local/bin/steam-shader-guard scan 2357570 --source /path/to/backup/nvidiav1
~/.local/bin/steam-shader-guard recover 2357570 --source /path/to/backup/nvidiav1 --apply
```

Choose the NVIDIA cache folder, normally containing `GLCache`, not an entire Steam
library. Shader data belongs to a particular driver and device. Do not copy it
between unrelated configurations or distribute someone else's cache.

### Saved files

| Item | Default location |
| --- | --- |
| Installed tool | `~/.local/bin/steam-shader-guard` |
| Steam menu shortcut | `~/.local/share/applications/steam-shader-guard.desktop` |
| A game's shader files | `~/.local/share/steam-shader-guard/games/<game-id>/nvidia/` |
| List of copied shader files and copy report | Next to the game's `nvidia` folder |
| Information used to undo changes | `~/.local/state/steam-shader-guard/state.json` |

`XDG_DATA_HOME` and `XDG_STATE_HOME` select other data and settings folders.
Unset or empty values use the defaults above.
`SHADER_GUARD_HOME` selects an isolated home for tests without changing `HOME`.
The game launcher requests a 12 GB NVIDIA cache unless an existing setting
specifies another value. This is not a limit on the total folder size.

Interrupted copying does not save an incomplete destination.
A forced stop can leave a hidden `.shader-guard-*` temporary folder beside the
destination. Remove it only after confirming no copy operation is running.

### Reports for scripts

Readable output is the default. Add `--json` to `doctor`, `scan` or `recover` for
the detailed JSON report:

```sh
~/.local/bin/steam-shader-guard scan 2357570 --json > scan.json
```

Progress goes to stderr and JSON goes to stdout.
Setup and undo commands print their plans and results on stdout.
Invalid arguments exit with status 2 before file changes. Other errors exit
with status 1. Repeated or unsupported options are errors.

`run` and `steam` pass arguments, including `--help` and `--version`, to the
started program. A first standalone `--` separates options from those arguments.
Later `--` values and non-UTF-8 arguments stay unchanged.
Steam settings keys are matched without regard to letter case.

Future Steam versions may change the pre-processing control.
To check it, look in Steam's `shader_log.txt` for
`Replay currently disabled on NVIDIA.` after using the new shortcut.

</details>

<details>
<summary>Building, testing and publishing releases</summary>

These instructions are for contributors. They require a source checkout.
Building needs Rust 1.85 or newer and Cargo.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

For the static Linux x86-64 executable:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
```

Tests use temporary folders, not your Steam settings, and do not need a GPU.
CLI tests need a non-root user and no Steam or Wine processes in their process
namespace. Where unprivileged user/PID namespaces are available:

```sh
unshare --user --map-current-user --pid --fork --mount-proc cargo test --locked
```

CI checks formatting, Clippy, Rust tests, installer syntax and Python tooling tests.
It runs Rust 1.99.0 on GNU and musl x86-64 Linux for pull requests, pushes to
`main` and manual runs. The declared Rust 1.85 minimum is not tested in CI.

### Releases

A tag matching the package version, such as `v0.2.0`, publishes a release after
CI passes. Update the manifest, lockfile, changelog and relevant notices first.
Prerelease tags such as `v0.2.0-rc.1` require the matching manifest version.
They are not marked as the latest stable release.
Release reruns do not overwrite existing releases.

Packages contain a static executable, documentation and license notices.
The release provides a versioned archive, the standalone `steam-shader-guard`
executable and `SHA256SUMS`. The installer checks this standalone download.
The packager checks executable linking and version information and generates
fresh checksums for downloads and archive contents.

To test packaging locally, use Python 3.11+ and `readelf` from binutils:

```sh
rustup toolchain install 1.99.0 --profile minimal --target x86_64-unknown-linux-musl
python3 -m unittest discover -s tests -p 'test_*.py' -v
RUSTUP_TOOLCHAIN=1.99.0 python3 scripts/release.py package v0.2.0
```

Replace `v0.2.0` with the package version. Files appear under `dist/`.
Packaging does not create a tag or publish a release.
Only the final publication job has repository write permissions.
External workflow actions are pinned to commit hashes.

</details>

## License

MIT. See [LICENSE](LICENSE) and [dependency notices](THIRD_PARTY_NOTICES.md).
The code is original and uses these
[cache-format notes](https://github.com/therontarigo/nvcachetools/blob/main/format.txt)
as documentation. This project is not affiliated with Valve or NVIDIA.
Downloads contain no personal Steam settings, shader data or recordings.
