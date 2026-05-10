# Contributing

Thanks for considering a contribution to `wt`.

## Development Setup

```bash
git clone https://github.com/hoetaek/wt.git
cd wt
cargo test --locked --all-features
```

## Checks

Run these before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

If you change dependencies, make sure `Cargo.lock` is updated.

## Versioning

`wt` follows SemVer.

- Patch: bug fixes and internal changes
- Minor: new features or config schema additions
- Major: breaking CLI or config changes

The version is managed in `Cargo.toml`.

## Pull Requests

Keep changes focused. Include tests for behavior changes, especially when
touching command parsing, config loading, provider integrations, or worktree
setup behavior.
