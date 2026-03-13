# Environment Variables

Deployments have environment variables that get injected into their functions at runtime. Variables come in two types: **plain** (configuration data) and **secret** (credentials stored in cloud vault).

## Plain vs Secret

**Plain variables** are injected directly into function configuration:

```
LOG_LEVEL=debug
API_ENDPOINT=https://api.example.com
```

The runtime sees them as regular environment variables. Fast, simple, visible in deployment logs.

**Secret variables** go through the cloud vault:

```
API_KEY=sk_live_abc123
DATABASE_PASSWORD=supersecret
```

During deployment, secrets are written to the vault (AWS Secrets Manager, GCP Secret Manager, Azure Key Vault). At function startup, the runtime fetches them from the vault and sets them as environment variables. Never appear in logs or configs.

Why the distinction? Enterprise requirements. Cloud vaults provide audit trails, rotation capabilities, and isolation from infrastructure code.

## Deployment Flow

### Pending Phase

When deployment starts, preflights run. The `SecretsVaultMutation` adds:

- A `secrets` vault resource (frozen lifecycle)
- `vault/data-read` permission to all function profiles
- `vault/data-write` permission to management profile

The vault gets deployed during InitialSetup.

### Provisioning Phase

Before deploying functions:

```rust
// provisioning.rs

// Inject environment variables into stack
inject_environment_variables(&mut target_stack, &config)?;

// Sync secrets to vault
sync_secrets_to_vault(&stack_state, &client_config, &config, &mut runtime_metadata).await?;
```

**Injection** modifies function configs:
- Plain variables → added directly to function environment
- Secret variables → `ALIEN_SECRETS` env var with keys and hash

**Sync** writes secret values to the cloud vault:
- Uses `RuntimeMetadata.last_synced_env_vars_hash` to skip redundant syncs
- Only syncs if hash changed (idempotent across retries)

### Runtime Loading

When the function starts, the Alien runtime checks for `ALIEN_SECRETS`:

```rust
// alien-runtime/src/secrets.rs

let config: AlienSecretsConfig = serde_json::from_str(&env_var)?;
// config = { keys: ["API_KEY", "DATABASE_PASSWORD"], hash: "a3f2c1..." }

let vault = bindings_provider.load_vault("secrets").await?;

for secret_key in config.keys {
    let value = vault.get_secret(&secret_key).await?;
    std::env::set_var(&secret_key, &value);
}
```

After this, `process.env.API_KEY` works normally in application code.

## Function Targeting

Variables can target specific functions:

```json
{
  "name": "DATABASE_URL",
  "value": "postgres://...",
  "type": "secret",
  "targetFunctions": ["processor"]
}
```

Patterns:
- `null` → all functions
- `["api-handler"]` → exact match
- `["api-*"]` → prefix wildcard (suffix only)
- `["api-*", "worker"]` → any pattern matches

```rust
// helpers.rs - matches_function_pattern()

if pattern.ends_with('*') {
    let prefix = &pattern[..pattern.len() - 1];
    function_name.starts_with(prefix)
} else {
    function_name == pattern
}
```

## The Vault Resource

Every stack includes a `secrets` vault:

- **Lifecycle:** Frozen (deployed once during InitialSetup)
- **Always present:** Even without secrets (env vars can be added later)
- **Platform-specific:** AWS Secrets Manager, GCP Secret Manager, Azure Key Vault

The `SecretsVaultMutation` adds it during preflights:

```rust
// secrets_vault.rs

let vault = Vault::new("secrets".to_string()).build();
stack.resources.insert("secrets".to_string(), ResourceEntry {
    config: Resource::new(vault),
    lifecycle: ResourceLifecycle::Frozen,
    dependencies: Vec::new(),
});
```

### Platform Implementations

| Platform | Model | Naming |
|----------|-------|--------|
| AWS | Prefix over Secrets Manager | `${resourcePrefix}-secrets-${secretName}` |
| GCP | Prefix over Secret Manager | `${resourcePrefix}-secrets-${secretName}` |
| Azure | Azure Key Vault resource | Key Vault named `${resourcePrefix}-secrets` |

AWS and GCP don't create a standalone vault resource—the vault is just a naming convention. Azure creates an actual Key Vault.

### Vault Beyond Environment Variables

The `secrets` vault is auto-added for environment variables. Developers can also add explicit vaults for other purposes:

```typescript
const userSecretsVault = new alien.Vault("user-secrets").build()

const func = new alien.Function("my-function")
  .link(userSecretsVault)
  .build()
```

Functions access vault contents via the bindings API:

```rust
let vault = ctx.bindings.load_vault("user-secrets").await?;
let secret = vault.get_secret("api-key").await?;
```

## Developer Secrets vs Customer Secrets

Two different flows for secrets:

### Developer Secrets (Environment Variables)

Secrets the developer controls. Flow through the control plane:

```
Developer → Control Plane → Vault → Deployment
```

Used for: API keys the developer manages, credentials for the developer's backend services.

### Customer Secrets (User Vault)

Secrets the customer controls. Bypass the control plane entirely:

```
Customer → Vault (directly) → Deployment
```

Used for: Customer's GitHub keys, database credentials, third-party API keys the customer doesn't want to share.

**How it works:**

1. Developer adds a user vault to the stack:
```typescript
const userSecretsVault = new alien.Vault("user-secrets").build()
```

2. During deployment, the vault is created (frozen resource)

3. Customer creates secrets directly in their cloud (AWS console, CLI, Terraform):
```bash
aws secretsmanager create-secret \
  --name "k44e9b72-user-secrets-github-key" \
  --secret-string "ghp_xxx"
```

4. Deployment loads secrets via bindings:
```rust
let vault = ctx.bindings.load_vault("user-secrets").await?;
let github_key = vault.get_secret("github-key").await?;
```

**Works across all deployment models:**

| Model | Why it works |
|-------|--------------|
| Push | Customer has direct access to their cloud's vault |
| Pull | Customer has direct access to their cloud's vault |
| Airgapped | Customer is in the same airgapped network as the vault |

Customer secrets never touch the control plane or sync protocol. The vault is infrastructure; the secrets are data the customer manages directly.

## Idempotent Secret Syncing

Deployments retry. Each retry shouldn't re-sync all secrets.

`RuntimeMetadata.last_synced_env_vars_hash` tracks what's been synced in the current deployment session:

```rust
// helpers.rs - sync_secrets_to_vault()

if runtime_metadata.last_synced_env_vars_hash == Some(snapshot.hash) {
    return Ok(false);  // Already synced
}

// ... sync secrets ...

runtime_metadata.last_synced_env_vars_hash = Some(snapshot.hash);
```
