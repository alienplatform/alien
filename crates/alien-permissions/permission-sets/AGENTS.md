## Guidelines for adding permission sets

1. To understand more about Alien's permission system, read `crates/alien-permissions/DESIGN.md`.
2. When adding a permission set, make sure all variables exist in the alien-permissions library, as currently we don't have compile-time validation for it.
3. Make sure all permissions / actions are accurate according to the cloud documentation.
4. Prefer two scopes in bindings: `stack` and `resource`.
   - AWS: use ARNs with `${stackPrefix}-*` and `${stackPrefix}-${resourceName}-*` patterns.
   - GCP: use project‑scoped CEL conditions on `resource.name.startsWith('projects/${projectName}/secrets/${stackPrefix}-')` etc.
   - Azure: use subscription/resourceGroup scoped paths with bare `${resourceName}`. What the token resolves to decides this: AWS callers pass the bare resource id, so an AWS binding has to name `${stackPrefix}-${resourceName}`, while every Azure caller passes the already-prefixed cloud name, so naming the prefix again renders it twice.
5. Use existing resources as references (e.g., `permission-sets/vault/*`).