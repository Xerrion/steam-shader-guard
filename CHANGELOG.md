# Changelog

## Unreleased

- Reject command-inappropriate arguments and duplicate options before any writes.
- Treat empty XDG directory variables as unset, using the documented defaults.

## 0.1.0 — 2026-10-06

- Initial Rust release for native Steam on Linux with NVIDIA.
- Strict CDVN validation, 4 GiB offset recovery and verified bounded output files.
- Steam library discovery, previews and opt-in per-game launch options.
- Independent Steam menu launcher, per-game caches and reversible installation.
- Unit and command-line integration tests; no runtime Python dependency.
