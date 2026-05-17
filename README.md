# WoVo

WoVo is a Tauri 2 + Leptos desktop app for monitoring Codex account usage, quota windows, account health, notifications, and account switching.

## Development

```sh
pnpm install
rustup target add wasm32-unknown-unknown
cargo install trunk
pnpm run tauri:dev
```

Useful checks:

```sh
pnpm run build:css
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

Build local production bundles with:

```sh
pnpm run tauri:build
```

## License

Apache-2.0. See [LICENSE](LICENSE).
