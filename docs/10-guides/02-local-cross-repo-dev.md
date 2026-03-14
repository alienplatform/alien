# Local cross-repo development

alien, deepstore, and horizon are separate repositories.  In production each repo consumes the
others through published packages (npm / crates.io).  When you need to iterate on multiple repos
simultaneously, you can redirect the deps to local checkouts.

## Recommended layout

```
~/repos/
  alien/       ← https://github.com/alienplatform/alien
  deepstore/   ← https://github.com/alienplatform/deepstore
  horizon/     ← https://github.com/alienplatform/horizon
```

---

## Rust: alien → horizon / deepstore

`Cargo.toml` in the alien workspace root contains a commented `[patch]` block at the bottom:

```toml
# --- Local development overrides ---
# Uncomment to use local checkouts instead of git tags.
# Do NOT commit this uncommented.
#
# [patch."https://github.com/alienplatform/horizon"]
# horizon-client-sdk = { path = "../horizon/crates/horizon-client-sdk" }
#
# [patch."https://github.com/alienplatform/deepstore"]
# deepstore-client = { path = "../deepstore/crates/deepstore-client" }
```

Uncomment the relevant lines, then `cargo build` will pick up the local source.

> **Do not commit** the uncommented `[patch]` sections — they will break CI.

---

## TypeScript: deepstore → alien

`deepstore/package.json` has a `pnpm.overrides` section that normally points at published npm
versions.  To use local alien packages instead:

1. Edit `deepstore/package.json`:

   ```json
   "pnpm": {
     "overrides": {
       "@alienplatform/core": "file:../alien/packages/core",
       "@alienplatform/typescript-config": "file:../alien/packages/config-typescript"
     }
   }
   ```

2. Run `pnpm install` in the deepstore root.

3. Restore before committing:

   ```json
   "pnpm": {
     "overrides": {
       "@alienplatform/core": "^0.1.0",
       "@alienplatform/typescript-config": "^0.1.0"
     }
   }
   ```

> **Do not commit** `file:../` overrides — they will break CI and other contributors.

---

## TypeScript: alien internal packages

The alien root `package.json` already uses `file:` overrides for packages within the alien monorepo
itself, so no extra steps are needed for intra-alien TypeScript development.
