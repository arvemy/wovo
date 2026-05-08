# Repository Guidelines

## Project Structure & Module Organization

This repository is a Tauri 2 desktop app with a Leptos/WASM frontend.

- `src/` contains the Leptos UI entry points, primarily `app.rs` and `main.rs`.
- `src-tauri/` contains the Rust backend, Tauri commands, domain models, Codex account/auth logic, capabilities, and app configuration.
- `style/tailwind.css` is the Tailwind input file; `styles.css` is the generated stylesheet consumed by `index.html`.
- `public/` stores static assets and logos.
- `Cargo.toml` defines the workspace; `src-tauri/Cargo.toml` defines the Tauri crate.

## Build, Test, and Development Commands

- `pnpm install` installs frontend/Tauri CLI dependencies using the pinned pnpm version.
- `pnpm run tauri:dev` starts the desktop app in development mode. Trunk serves the frontend on port `1420`.
- `pnpm run build:css` rebuilds `styles.css` from Tailwind.
- `pnpm run watch:css` watches Tailwind input while editing UI styles.
- `pnpm run tauri:build` creates a production Tauri build.
- `cargo test` runs Rust unit tests across the workspace.
- `cargo fmt --all` formats Rust code before submitting changes.
- `cargo clippy --workspace --all-targets` checks Rust code for common issues.

## Coding Style & Naming Conventions

Use standard Rust formatting with 4-space indentation through `rustfmt`. Prefer small modules grouped by concern: UI state in `src/app.rs`, domain types in `src-tauri/src/domain/`, Codex integration in `src-tauri/src/codex/`, and app errors in `src-tauri/src/error.rs`.

Rust functions and modules use `snake_case`; types and Leptos components use `PascalCase`. Serde API payloads use `#[serde(rename_all = "camelCase")]` where they cross the frontend/backend boundary.

## Testing Guidelines

Place Rust unit tests beside the code they cover in `#[cfg(test)] mod tests` blocks. Name tests after the behavior being protected, for example `prepare_home_promotes_materialized_state_without_deleting_it`. Run `cargo test` before opening a PR, and add focused tests for auth/account-store behavior or command serialization changes.

## Commit & Pull Request Guidelines

Recent commits use short, imperative subjects such as `Add managed Codex account support` and `Refactor app routing and layout system`. Keep subjects concise and describe the user-visible or architectural change.

PRs should include a brief summary, test results, linked issues when relevant, and screenshots or screen recordings for UI changes. Call out changes that affect stored account data, authentication flow, Tauri permissions, or generated assets like `styles.css`.

## Security & Configuration Tips

Do not commit local Codex credentials, OAuth tokens, generated app data, or build outputs from `target/`, `dist/`, or `node_modules/`. Review `src-tauri/capabilities/default.json` when adding new Tauri commands or permissions.
