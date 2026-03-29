# Local cross-repo development

When developing across packages within the alien monorepo, local `file:` overrides handle dependency resolution automatically.

## TypeScript: alien internal packages

The alien root `package.json` already uses `file:` overrides for packages within the alien monorepo
itself, so no extra steps are needed for intra-alien TypeScript development.

## Rust: local path overrides

`Cargo.toml` in the alien workspace root can use `[patch]` sections to redirect dependencies to local checkouts when developing against external crates:

```toml
# --- Local development overrides ---
# Uncomment to use local checkouts instead of published crates.
# Do NOT commit this uncommented.
#
# [patch.crates-io]
# some-crate = { path = "../some-crate" }
```

Uncomment the relevant lines, then `cargo build` will pick up the local source.

> **Do not commit** the uncommented `[patch]` sections — they will break CI.
