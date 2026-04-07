# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**stencil-but-fast** is a Rust rewrite of the BigCommerce Stencil CLI (`stencil-cli`). It provides a local development server for BigCommerce theme development with live reload, SCSS compilation, and API proxying.

- **Language:** Rust (2021 edition)
- **Binary name:** `stencil`
- **Main entry point:** `src/main.rs`

## Common Development Commands

```bash
# Build
cargo build               # Debug build
cargo build --release     # Release build

# Run
cargo run -- start        # Run dev server (debug)
cargo run -- init         # Initialize config

# Check & Lint
cargo check               # Fast type-check without full compile
cargo clippy              # Lint
cargo fmt                 # Format code

# Test
cargo test                # Run all tests
cargo test test_name      # Run a specific test by name pattern
```

## Architecture

### CLI Commands (`src/main.rs` + `src/commands/`)

Two commands parsed via `clap` derive macros:
- **`init`** — Interactive prompts to create `config.stencil.json` + `secrets.stencil.json`
- **`start`** — Loads theme, connects to BigCommerce, starts local Axum server

### Server (`src/server/`)

Axum-based server with route dispatch in `app.rs`:
- `/__live_reload` → WebSocket for browser hot reload
- `/graphql` → Proxy to BigCommerce GraphQL API
- `/stencil/*` + static extensions (`*.css`, `*.js`, images, fonts) → `theme_assets.rs` (SCSS compiled via `grass`)
- `/assets/*` → Tower filesystem handler
- `/internalapi/*`, `/api/storefront/*` → `proxy.rs` (API passthrough)
- **Fallback** → `renderer.rs` (Handlebars template rendering)

### Template Rendering Pipeline (`src/renderer/`)

1. `template_assembler.rs` — Recursively resolves `{{> partial}}` references and `external/` node_modules components
2. `frontmatter.rs` — Extracts YAML frontmatter from templates
3. `lang_assembler.rs` — Merges i18n JSON files
4. `paper.rs` — Renders via `handlebars` engine with live reload script injection

### Configuration (`src/config/`)

- `stencil_config.rs` — Loads `config.stencil.json` (store URL, port, API host) and `secrets.stencil.json` (OAuth token). Also handles migration from the legacy `.stencil` single-file format.
- `theme_config.rs` — Reads `config.json` from the theme directory for variations/settings.

### Shared State (`src/server/state.rs`)

`Arc<RwLock<AppState>>` passed to all handlers containing: BigCommerce API client, cache, theme config, live reload broadcast channel, and channel/store metadata.

### Key Supporting Modules

- `src/proxy/` — `reqwest`-based HTTP client; rewrites headers (cookies, auth, host) for BC API calls
- `src/cache/memory_cache.rs` — TTL-based in-memory cache for API responses; bypassed with `--no_cache`
- `src/watcher/file_watcher.rs` — `notify` + debouncer; sends `LiveReloadMessage::FullReload` or `CssReload` on file changes

## Configuration Files

| File | Purpose |
|------|---------|
| `config.stencil.json` | Store URL, local port, API host, custom layouts |
| `secrets.stencil.json` | OAuth access token, optional GitHub token |
| `config.json` (theme dir) | Theme variations and settings |

## Key Dependencies

| Crate | Use |
|-------|-----|
| `axum` 0.7 | Web framework |
| `tokio` | Async runtime |
| `reqwest` 0.12 | HTTP client for BC API |
| `handlebars` 6 | Template rendering |
| `grass` 0.13 | SCSS → CSS compilation |
| `notify` 6 | File watching |
| `clap` 4 | CLI argument parsing (derive) |
| `serde`/`serde_json`/`serde_yaml` | Config serialization |
| `anyhow`/`thiserror` | Error handling |
| `tracing` | Structured logging |
