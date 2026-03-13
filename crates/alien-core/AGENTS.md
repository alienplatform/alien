# alien-core

Core types and abstractions for the Alien platform.

## Type Generation & Code Sync

Types in this crate are exported to TypeScript for use in `@alienplatform/core`.

### Quick Reference

```bash
# After modifying Rust types, regenerate TypeScript
pnpm generate  # Rust → OpenAPI → Zod schemas
pnpm build
```

### Architecture

```
Rust types (alien-core)
  ↓ schema_exporter.rs (utoipa)
OpenAPI JSON
  ↓ Kubb (packages/core/kubb.config.ts)
Zod schemas + TypeScript types
  ↓ Published as @alienplatform/core
```

### Adding New Types

1. **Define type in Rust** with `#[derive(Serialize, Deserialize)]` and `#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]`
2. **Add to schema_exporter.rs**: Include in `#[openapi(components(schemas(...)))]` list
3. **Generate & export**: Run `pnpm generate && pnpm build` in `alien/`
4. **Export from TypeScript**: Add to `packages/core/src/index.ts` exports

