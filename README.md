# WoVo

WoVo is a Tauri 2 desktop app for tracking Codex account usage. It shows quota windows, cost usage, account health, notifications, and account switching controls from a Leptos/WASM frontend backed by a Rust Tauri service.

## Development

Install dependencies:

```sh
pnpm install
```

Start the desktop app in development mode:

```sh
pnpm run tauri:dev
```

Build CSS after changing Tailwind classes:

```sh
pnpm run build:css
```

Run Rust checks:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

Build release bundles:

```sh
pnpm run tauri:build
```

## License

WoVo is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
