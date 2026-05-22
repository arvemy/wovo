# Repository Guidelines

## Project Layout

WoVo is a Tauri 2 desktop app with a Leptos/WASM frontend.

- `src/` contains the Leptos app, entry points, API bindings, and frontend state.
- `src/ui/` contains reusable Rust/UI primitives; export new shared controls from `src/ui/mod.rs`.
- `src-tauri/` contains the Rust backend, Tauri commands, Codex and Claude account/auth logic, domain models, capabilities, and app config.
- `style/tailwind.css` is the Tailwind source; `styles.css` is generated and loaded by `index.html`.
- `public/` stores static assets and logos.
- Root `Cargo.toml` defines the frontend package/workspace; `src-tauri/Cargo.toml` defines the Tauri crate.

## Common Commands

- `pnpm install` installs frontend and Tauri CLI dependencies.
- `pnpm run tauri:dev` starts the desktop app; Trunk serves the frontend on port `1420`.
- `pnpm run build:css` regenerates `styles.css` after Tailwind class changes.
- `pnpm run watch:css` watches Tailwind input while editing UI.
- `pnpm run tauri:build` creates production Tauri bundles.
- `cargo fmt --all` formats Rust code.
- `cargo clippy --workspace --all-targets -- -D warnings` checks Rust code with warnings denied.
- `cargo test --workspace` runs Rust tests.

## Rust Style

Use standard `rustfmt` formatting. Prefer small modules grouped by concern: UI state in `src/app.rs` and `src/views/`, domain types in `src-tauri/src/domain/`, provider integrations in `src-tauri/src/codex/` and `src-tauri/src/claude/`, and shared errors in `src-tauri/src/error.rs`.

Use `snake_case` for functions/modules and `PascalCase` for types and Leptos components. Payloads crossing the frontend/backend boundary should use `#[serde(rename_all = "camelCase")]`.

Place focused Rust tests beside the code they cover in `#[cfg(test)] mod tests` blocks. Name tests after the behavior being protected, for example `prepare_home_promotes_materialized_state_without_deleting_it`.

## Frontend Components

Use the existing Rust/UI component layer in `src/ui/` before adding one-off markup in feature views. Keep shared component APIs small, typed, and consistent with nearby call sites.

Follow the local `leptos_ui::variants` and `leptos_ui::clx` patterns used by `button.rs`, `badge.rs`, `card.rs`, and related modules. If a component needs variants, use enum-based variants instead of stringly typed props.

Prefer the `icons` crate for common interface icons:

```rust
use icons::{RefreshCw, Trash2};

view! {
    <RefreshCw class="size-4" />
    <Trash2 class="size-4 text-destructive" />
}
```

Do not add inline SVGs for common icons when an `icons` component exists. Icon-only buttons should follow the local button sizing pattern and include accessible text through labels, titles, or ARIA attributes.

When using the Rust/UI CLI, install or update it with `cargo install ui-cli --force`, then use commands such as `ui add button`, `ui view <name>`, `ui diff`, or `ui update`. Do not run `ui init` unless intentionally reconfiguring the project. If the CLI generates `src/components/`, adapt the generated code into `src/ui/` rather than keeping a second component tree.

## Generated Files

Run `pnpm run build:css` after changing Tailwind classes and commit the resulting `styles.css` update when needed. Inspect generated diffs before committing dependency, component, or style changes.

## Security And Configuration

Do not commit local Codex or Claude credentials, OAuth tokens, generated app data, or build outputs from `target/`, `dist/`, or `node_modules/`. Review `src-tauri/capabilities/default.json` when adding Tauri commands or permissions.

Release versions must stay aligned across `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, root `Cargo.toml`, and `package.json`. Releases are produced from `v*.*.*` tags and are left as draft GitHub Releases for maintainer review.

## Commits And PRs

Use short, imperative commit subjects such as `Refine provider UI controls` or `Add Claude quota notifications`.

PRs should include a brief summary, test results, linked issues when relevant, and screenshots or recordings for UI changes. Call out changes that affect stored account data, authentication flow, Tauri permissions, release signing, updater behavior, or generated assets.
