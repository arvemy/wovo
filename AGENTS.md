# Repository Guidelines

## Project Structure & Module Organization

This repository is a Tauri 2 desktop app with a Leptos/WASM frontend.

- `src/` contains the Leptos UI entry points and components, including `app.rs`, `main.rs`, and reusable UI modules in `src/ui/`.
- `src-tauri/` contains the Rust backend, Tauri commands, domain models, Codex account/auth logic, capabilities, and app configuration.
- `style/tailwind.css` is the Tailwind input file; `styles.css` is the generated stylesheet consumed by `index.html`.
- `public/` stores static assets and logos.
- `Cargo.toml` defines the workspace; `src-tauri/Cargo.toml` defines the Tauri crate.

## Build, Test, and Development Commands

- `pnpm install` installs frontend and Tauri CLI dependencies.
- `pnpm run tauri:dev` starts the desktop app in development mode. Trunk serves the frontend on port `1420`.
- `pnpm run build:css` rebuilds `styles.css` from Tailwind.
- `pnpm run watch:css` watches Tailwind input while editing UI styles.
- `pnpm run tauri:build` creates a production Tauri build.
- `cargo test` runs Rust unit tests across the workspace.
- `cargo fmt --all` formats Rust code before submitting changes.
- `cargo clippy --workspace --all-targets` checks Rust code for common issues.

## Coding Style & Naming Conventions

Use standard Rust formatting with 4-space indentation through `rustfmt`. Prefer small modules grouped by concern: UI state in `src/app.rs`, domain types in `src-tauri/src/domain/`, Codex integration in `src-tauri/src/codex/`, and errors in `src-tauri/src/error.rs`.

Rust functions and modules use `snake_case`; types and Leptos components use `PascalCase`. Serde API payloads should use `#[serde(rename_all = "camelCase")]` when they cross the frontend/backend boundary.

## Frontend UI Components & Icons

Use the existing Rust/UI-style component layer in `src/ui/` before creating one-off markup in `src/app.rs`. Shared controls should live in `src/ui/<component>.rs`, be exported from `src/ui/mod.rs`, and follow the local `leptos_ui::variants` or `leptos_ui::clx` patterns already used by `button.rs`, `badge.rs`, `card.rs`, and related modules.

When adding Rust/UI components from the CLI, install or update the tool with `cargo install ui-cli --force`, then use `ui add <component>` for specific components. Avoid running `ui init` unless intentionally reconfiguring the project, because this repo already has Tailwind, Leptos, and a local `src/ui/` structure. The CLI may generate a default `src/components/` layout; adapt generated code into `src/ui/` instead of introducing a second component directory. Inspect generated diffs for dependency or style changes before committing.

Prefer Rust/UI and local `src/ui/` components for buttons, badges, cards, checkboxes, progress, separators, tooltips, and new reusable primitives. Keep component APIs small, typed, and consistent with existing call sites. If a component needs variants, use the same enum-based variant style rather than stringly typed props.

Use the `icons` crate for frontend icons. It provides Lucide-style Leptos components, and this workspace already enables it with the `leptos` feature in `Cargo.toml`. Import named icons directly, for example `use icons::{RefreshCw, Trash2};`, and size/color them with Tailwind classes:

```rust
view! {
    <RefreshCw class="size-4" />
    <Trash2 class="size-4 text-destructive" />
}
```

Do not add inline SVGs for common interface icons when an `icons` crate component exists. Icon-only buttons should use the local button icon sizing pattern and include accessible text through surrounding labels, titles, or ARIA attributes as appropriate. Keep icon sizes consistent with nearby UI: `size-3.5` or `size-4` for dense controls, `size-6` or larger only where the layout calls for it.

After adding or changing Tailwind classes in components, run `pnpm run build:css` so `styles.css` stays current.

## Testing Guidelines

Place Rust unit tests beside the code they cover in `#[cfg(test)] mod tests` blocks. Name tests after the behavior being protected, for example `prepare_home_promotes_materialized_state_without_deleting_it`. Run `cargo test` before opening a PR, and add focused tests for auth/account-store behavior or command serialization changes.

## Commit & Pull Request Guidelines

Recent commits use short, imperative subjects such as `Refine Codex UI controls` and `Add Codex quota notifications`. Keep subjects concise and describe the user-visible or architectural change.

PRs should include a brief summary, test results, linked issues when relevant, and screenshots or screen recordings for UI changes. Call out changes that affect stored account data, authentication flow, Tauri permissions, or generated assets like `styles.css`.

## Security & Configuration Tips

Do not commit local Codex credentials, OAuth tokens, generated app data, or build outputs from `target/`, `dist/`, or `node_modules/`. Review `src-tauri/capabilities/default.json` when adding new Tauri commands or permissions.
