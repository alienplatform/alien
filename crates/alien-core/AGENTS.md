- Types here are exported to TypeScript via OpenAPI → Zod. New types need `#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]` and must be added to `schema_exporter.rs`.
- After modifying types: `pnpm generate && pnpm build` from workspace root.
- Use "deployment" not "agent", "commands" not "ARC", "manager" not "server".

### `src/import/`

`alien-core` owns **only** the request shape for distribution import:

- `data/<cloud>/<resource>.rs` — typed `Aws*ImportData` / `Gcp*ImportData` / `Azure*ImportData` structs. Pure data + JsonSchema, camelCase serde. No methods.
- `request.rs` — `StackImportRequest` / `ImportedResource` / `StackImportResponse` / `ImportSourceKind`. The HTTP request / response types of `POST /v1/stack/import`.
- `context.rs` — `EmitContext` (passed to format emitters) and `ImportContext` (passed to importers). Pure data; format-agnostic.

Format emitters live in their format crates and use that format's native types directly:

- `alien_cloudformation::CfEmitter` + `CfRegistry`. `CfResource` / `CfExpression` live here, not in alien-core.
- `alien_terraform::TfEmitter` + `TfRegistry`. Returns `hcl::Block` / `hcl::Expression` from `hcl-rs` directly — no `TfBlock` / `TfExpression` IR.
- `alien_helm::HelmEmitter` + `HelmRegistry`. Templates as Go-templated YAML.

Importers live in `alien-infra` next to the controllers (since they produce controller state):

- `alien_infra::ResourceImporter` trait, registered into `ImporterRegistry::built_in()`.
- Per-resource impls at `alien-infra/src/<resource>/<cloud>_import.rs`.

When changing any `*ImportData` field, run the schema snapshot under `import/data/snapshots/` (`cargo test -p alien-core --features jsonschema --lib import`) and review the diff — every distribution adapter and importer needs to know.

`alien-core` MUST NOT depend on `alien-infra` or any format crate.
