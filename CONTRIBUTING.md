# Contributing

Thanks for helping improve Diffloom.

## Quick start

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

On Linux, you may need development libraries for the GUI stack (winit / GL), for example:

```bash
sudo apt-get install -y pkg-config libx11-dev libxi-dev libxcb1-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libgl1-mesa-dev
```

## Pull requests

- Keep changes focused on one concern when possible.
- Run `cargo fmt` and ensure `cargo clippy --all-targets -- -D warnings` passes (CI enforces this).
- Add or extend tests when behavior changes.

## Repository metadata

If you fork or republish, update `repository` / `homepage` in `Cargo.toml` and any badge URLs in `README.md` to match your GitHub org or user.

## Code of conduct

All contributors are expected to follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Security

See [SECURITY.md](SECURITY.md) for how to report vulnerabilities.

## Publishing to crates.io

Maintainers can run the **Publish crate** workflow from the Actions tab. Configure a repository secret named `CRATES_IO_TOKEN` with a [crates.io API token](https://crates.io/settings/tokens) that has `publish-update` scope for the `diffloom` crate.
