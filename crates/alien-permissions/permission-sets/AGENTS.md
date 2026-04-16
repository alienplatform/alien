## Guidelines for adding permission sets

1. To understand more about Alien's permission system, read `crates/alien-permissions/DESIGN.md`.
2. When adding a permission set, make sure all variables exist in the alien-permissions library, as currently we don't have compile-time validation for it.
3. Make sure all permissions / actions are accurate according to the cloud documentation.
4. Prefer two scopes in bindings: `stack` and `resource`.
   - AWS: use ARNs with `${stackPrefix}-*` and `${stackPrefix}-${resourceName}-*` patterns.
   - GCP: use project‑scoped CEL conditions on `resource.name.startsWith('projects/${projectName}/secrets/${stackPrefix}-')` etc.
   - Azure: use subscription/resourceGroup scoped paths with `${stackPrefix}-${resourceName}`.
5. Use existing resources as references (e.g., `permission-sets/vault/*`).