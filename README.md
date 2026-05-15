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

## Releases

Release builds are published from SemVer tags such as `v0.1.1`. Before tagging, update the version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, root `Cargo.toml`, and `package.json` to the same value, then run:

```sh
node scripts/validate-release-version.mjs v0.1.1
pnpm run build:css
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The release workflow builds macOS, Windows, and Linux bundles, uploads Tauri updater metadata to GitHub Releases, and leaves the GitHub release as a draft for maintainer review. Auto-update signing uses the public key committed in `src-tauri/tauri.conf.json`; keep the private key out of the repository and store it in the GitHub secret `TAURI_SIGNING_PRIVATE_KEY`. If the key has a password, store it in `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

On newer Linux distributions, AppImage bundling may need `NO_STRIP=true` because the bundled `linuxdeploy` strip tool can fail on modern `.relr.dyn` ELF sections.

## License

WoVo is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
