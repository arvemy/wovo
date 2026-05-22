# WoVo

WoVo is a Tauri 2 + Leptos desktop app for monitoring Codex and Claude Code account usage. It tracks quota windows, account health, local token costs, notifications, and account switching from one desktop UI.

## Features

- Codex and Claude Code provider views
- Managed and detected account listing with reauthentication, removal, and system-account switching
- OAuth, CLI, and automatic usage-source modes
- Quota-window cards, local cost tracking, and stale snapshot indicators
- Quota and auto-switch notifications
- Optional launch-on-login, tray behavior, and app updates

## Development

Install the JavaScript and Rust tooling, then start the Tauri app:

```sh
pnpm install
rustup target add wasm32-unknown-unknown
cargo install trunk
pnpm run tauri:dev
```

Trunk serves the Leptos frontend on port `1420` while Tauri runs the desktop shell.

## Checks

```sh
pnpm run build:css
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run `pnpm run build:css` after changing Tailwind classes because `styles.css` is generated from `style/tailwind.css`.

## Build

```sh
pnpm run tauri:build
```

Release versions must stay aligned across `package.json`, both Cargo manifests, and `src-tauri/tauri.conf.json`.

## License

Apache-2.0. See [LICENSE](LICENSE).
