# CLAUDE.md

## Build and test

Keep output small: quiet flags first, full output only for what failed.

- Tests: `cargo nextest run` (quiet profile in `.config/nextest.toml`: only failures print).
  Narrow with `-p <crate>` or a test-name filter.
- Never use `cargo test`, except `cargo test --doc` for doctests (nextest doesn't run them).
- Lint: `cargo clippy --all-targets --message-format=short -q -- -D warnings`
- Build check: `cargo check --message-format=short -q`
- On failure only, re-run just the failing test or crate with full output
  (e.g. `cargo nextest run -p sim <test_name> --no-capture`). Don't dump full build logs.
- When piping cargo output through `tail`/`grep`, run `set -o pipefail` first so the exit code survives.
- Backtraces are off (`RUST_BACKTRACE=0` in `.claude/settings.json`). To debug a panic, enable
  them for that one command: `RUST_BACKTRACE=1 cargo nextest run <test_name>`.
- Never run `cargo clean` unless asked.
- Don't read `target/`. `out/` and `screenshots/` hold generated renders/logs: open specific files only.
