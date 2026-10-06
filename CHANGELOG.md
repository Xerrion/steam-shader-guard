# Changelog

## 0.2.0 (2026-10-06)

- Install or update with a curl command on x86-64 Linux. The installer checks the
  downloaded program before running it. It does not change game settings.
- Rewrite both guides around setting up a game, with explanations of game IDs,
  optional shader copying and the new Steam shortcut.
- Show readable game lists and shader checks by default. Add `--json` to
  `doctor`, `scan` or `recover` for reports used by scripts.
- Explain what each command is doing, why it stopped and what to do next.
  Game setup reports how many games it changed and which ones it left unchanged.
- Offer both a standalone program and an archive with guides and license notices.
  Downloads do not include source code or build tools.
- Name the standalone program `steam-shader-guard`.
- Provide checksums for both downloads so users can check the files.

## 0.1.0 (2026-10-06)

- Initial Rust release for native Steam on Linux with NVIDIA.
- Strict CDVN validation, 4 GiB offset recovery and verified bounded output files.
- Steam library discovery, previews and opt-in per-game launch options.
- Independent Steam menu launcher, per-game caches and reversible installation.
- Unit and command-line integration tests; no runtime Python dependency.
- Replace custom CLI parsing with Clap, including generated command help,
  command-specific validation and argument errors with exit status 2.
- Preserve OS-native paths and forwarded game/Steam arguments. An initial `--`
  is a CLI separator for both forwarding commands.
- Reject command-inappropriate arguments and duplicate options before any writes.
- Match Steam VDF keys case-insensitively and reject case-variant duplicate keys.
- Keep the executable when uninstall cannot inspect known accounts or menu entries.
- Keep the executable when a symlinked menu entry still references it, even if its
  contents match the originally installed menu entry.
- Preserve both installed and pending hashes so interrupted installation updates
  can be retried or uninstalled safely.
- Recognize equivalent Steam cache paths containing trailing or repeated slashes
  and `.` components instead of bypassing cache isolation.
- Reject recovery destinations inside the source before creating directories.
- Treat empty XDG directory variables as unset, using the documented defaults.
- Add regression tests demonstrating the failures and covering interrupted-journal
  recovery; document process-isolated CLI testing.
- GitHub Actions CI on Rust 1.99.0 for formatting, Clippy and GNU/musl tests.
- Version-tag releases with verified static executables, documentation, notices and
  regenerated SHA-256 manifests; prerelease tags are supported.
