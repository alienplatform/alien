# Isolated Azure storage controller qualification

These ignored tests require an explicitly provisioned disposable resource group,
StorageV2 account, fresh runtime identity, and separate short-lived installer and
runtime ARM token files. The fixture owner must clean up exact recorded resources
and role definitions even if a test fails. Never point this at an installed
account. Keep the directory mode0700 and tokens mode0600; do not archive tokens.

Set `ALIEN_AZURE_STORAGE_FIXTURE` to a JSON file containing `subscriptionId`,
`tenantId`, `region`, `prefix` (starting `test-`), `resourceGroup` (starting with
that prefix), `account`, `setupTokenFile`, `runtimeTokenFile`, and `evidenceDir`.
The installer needs create/update access only for its dedicated fixture and role
setup. The runtime identity must start with no assignments. A disposable managed
identity with an isolated OIDC issuer is sufficient; no VM is needed.

Build with `cargo test -p alien-infra --all-features --test azure_storage_live
--no-run`. Execute `export_grants -- --ignored` first. Install **exactly** the
exported `grants.json` plans on the fresh runtime identity at the dedicated
resource-group scope. Preserve their actions, data actions and assignment scope;
use unique role-definition IDs and role names for the fixture. Prove the runtime
identity is denied before granting access, then wait for bounded RBAC propagation on every required metadata operation
(account, blob service, and management policy). Account read alone can succeed
through the custom role while the separate Reader assignment is still propagating.
Do not give it Owner, Contributor, account keys, or data-read roles.

Run `controller_phase -- --ignored --nocapture --test-threads=1` with
`ALIEN_AZURE_STORAGE_PHASE` set to these phases in order:

1. `setup`: installer runs the real shared account controller with Live/Frozen
   contributors. Verify versioning, GET/HEAD CORS and two disabled scoped rules (assets/logs and sibling/backup). Seed the Frozen sibling container under installer authority.
2. `create`: runtime creates the declared public container and verifies shared
   settings under the generated grants. The installer can seed two versions of
   a synthetic object at `PREFIX-assets/proof.txt` using its own data authority.
   Verify anonymous GET, HEAD, Range and exposed CORS response headers.
3. `private`: runtime updates public access. Independently verify anonymous denial
   and installer readability of the original synthetic data/versions.
4. Explicitly enable both generated expiry rules as the installer, with no
   objects in their prefixes. `remove-expiry` must disable only the assets rule
   while preserving the enabled sibling rule. `remove-all-expiry` disables the
   final sibling rule; `verify-removal` checks the empty assets declaration
   through the runtime controller.
   This tests configuration, **not** completion of Azure's expiry propagation.
5. Remove exact generated runtime assignments. Use an already-issued runtime
   token to verify metadata denial while the installer still reads the same
   object. `erase` is optional while runtime grants remain; otherwise explicit
   installer teardown owns erasing the dedicated account and all versions.

Cleanup must remove all fixture role assignments, custom role definitions,
managed identity/federation, containers (including OIDC public metadata), and
storage account/resource group. Verify exact-ID absence independently and erase
local tokens. A role assignment delete response alone is not effective-access
proof, and a policy readback is not a claim that scheduled expiry has stopped.
