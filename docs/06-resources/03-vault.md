# Vault

Vault stores secrets using cloud-native secret management services. On AWS and GCP, it's a naming convention over the existing service. On Azure, it creates an actual Key Vault resource.

## Platform Mapping

| Platform | Backend | Model |
|----------|---------|-------|
| AWS | Secrets Manager | Prefix-based naming |
| GCP | Secret Manager | Prefix-based naming |
| Azure | Key Vault | Actual resource |
| Local | File-based | Directory per vault |

## Two Types of Vaults

### 1. Secrets Vault (Auto-Generated)

Every stack gets a `secrets` vault for environment variable storage. The `SecretsVaultMutation` preflight adds it automatically:

```rust
// SecretsVaultMutation adds this to every stack
let vault = Vault::new("secrets".to_string()).build();
stack.resources.insert("secrets", ResourceEntry {
    config: Resource::new(vault),
    lifecycle: ResourceLifecycle::Frozen,  // Created once
    dependencies: Vec::new(),
});
```

This vault stores secret-type environment variables. See [Environment Variables](../01-provisioning/03-environment-variables.md) for details.

### 2. User Vaults (Developer-Defined)

Developers can add explicit vaults for other purposes:

```typescript
const userSecretsVault = new alien.Vault("user-secrets").build()

const func = new alien.Function("my-function")
  .link(userSecretsVault)
  .build()
```

## Naming Convention

Secret names follow a consistent pattern: `{stackPrefix}-{vaultName}-{secretName}`

| Platform | Example Secret Name |
|----------|---------------------|
| AWS | `k44e9b72-secrets-API_KEY` |
| GCP | `k44e9b72-secrets-API_KEY` |
| Azure | Key Vault `k44e9b72-secrets`, secret `API_KEY` |

Azure is different—it creates a Key Vault resource named `{stackPrefix}-{vaultName}` and stores secrets inside it.

## Controller Behavior

### AWS Controller

AWS Secrets Manager exists implicitly. The controller stores the vault prefix and returns immediately:

```rust
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Vault>()?;
    
    self.vault_prefix = Some(format!("{}-{}", ctx.resource_prefix, config.id));
    self.account_id = Some(aws_cfg.account_id.to_string());
    self.region = Some(aws_cfg.region.clone());
    
    Ok(HandlerAction::Continue { state: Ready, .. })
}
```

No infrastructure created—just prefix tracking.

### GCP Controller

Same pattern as AWS. Secret Manager exists implicitly:

```rust
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    self.vault_prefix = Some(format!("{}-{}", ctx.resource_prefix, config.id));
    self.project_id = Some(gcp_cfg.project_id.clone());
    self.location = Some(gcp_cfg.region.clone());
    
    Ok(HandlerAction::Continue { state: Ready, .. })
}
```

### Azure Controller

Azure creates an actual Key Vault resource:

```rust
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    self.vault_name = Some(format!("{}-{}", ctx.resource_prefix, config.id));
    
    // Create the Key Vault
    let vault_params = VaultCreateOrUpdateParameters {
        location: azure_config.region,
        properties: VaultProperties {
            sku: Sku { name: SkuName::Standard, family: SkuFamily::A },
            tenant_id,
            access_policies,  // Initial policies for management principal
            enable_soft_delete: true,
            soft_delete_retention_in_days: 7,
            ..
        },
        tags: HashMap::from([("ManagedBy", "Alien")]),
    };
    
    client.create_or_update_vault(resource_group, vault_name, vault_params).await?;
    
    Ok(HandlerAction::Continue { state: ApplyingPermissions, .. })
}
```

After creation, the controller applies resource-scoped permissions for function profiles.

## Bindings

The controller generates binding parameters for runtime access:

```rust
fn get_binding_params(&self) -> Option<serde_json::Value> {
    let binding = match platform {
        Aws => VaultBinding::parameter_store(vault_prefix),
        Gcp => VaultBinding::secret_manager(vault_prefix),
        Azure => VaultBinding::key_vault(vault_name),
    };
    serde_json::to_value(binding).ok()
}
```

### Binding Types

```rust
pub enum VaultBinding {
    SecretsManager(SecretsManagerVaultBinding),  // AWS
    SecretManager(SecretManagerVaultBinding),     // GCP
    KeyVault(KeyVaultBinding),                    // Azure
    Local(LocalVaultBinding),                     // Development
}
```

## Binding API

Functions access secrets through the bindings API:

```rust
let vault = ctx.get_bindings().load_vault("user-secrets").await?;

// Read
let db_password = vault.get_secret("database-password").await?;

// Write (if function has vault/data-write permission)
vault.set_secret("api-key", &new_api_key).await?;

// Delete
vault.delete_secret("old-api-key").await?;
```

## Permissions

Vault permissions control who can read, write, and manage secrets.

### Permission Sets

| Permission | Purpose |
|------------|---------|
| `vault/data-read` | Read secrets at runtime |
| `vault/data-write` | Create, update, delete secrets |
| `vault/management` | Vault metadata access |
| `vault/provision` | Create/delete vaults (Azure only) |

### Auto-Added Permissions

The `SecretsVaultMutation` adds:

1. `vault/data-read` to all function profiles (for loading env var secrets at runtime)
2. `vault/data-write` to management profile (for syncing secrets during deployment)

```rust
// Added to every function profile
profile.add("*", vec![PermissionSetReference::from_name("vault/data-read")]);

// Added to management profile
management.add("*", vec![PermissionSetReference::from_name("vault/data-write")]);
```

### Manual Configuration

For user vaults, configure permissions explicitly:

```typescript
.permissions({
  profiles: {
    executor: {
      "user-secrets": ["vault/data-read"],      // Function can read
    }
  },
  management: {
    extend: {
      "user-secrets": ["vault/data-write"],     // Control plane can write
    }
  }
})
```

## Developer vs Customer Secrets

Two different flows for secrets:

### Developer Secrets

Secrets the developer controls, stored via environment variables API:

```
Developer → API → Platform DB (encrypted) → DeploymentConfig → Vault → Agent
```

Used for: API keys, backend service credentials managed by the developer.

### Customer Secrets

Secrets the customer controls, stored directly in the cloud vault:

```
Customer → Vault (directly) → Agent
```

The customer creates secrets using their cloud console or CLI:

```bash
aws secretsmanager create-secret \
  --name "k44e9b72-user-secrets-github-key" \
  --secret-string "ghp_xxx"
```

Functions access them via bindings:

```rust
let vault = ctx.bindings.load_vault("user-secrets").await?;
let github_key = vault.get_secret("github-key").await?;
```

This flow works for all deployment models (Push, Pull, Airgapped) because customers always have direct access to their own vault.

## Environment Variables Integration

The `secrets` vault is tightly integrated with environment variables. See [Environment Variables](../01-provisioning/03-environment-variables.md) for:

- How secret-type env vars flow through the deployment
- Encryption at rest (AES-256-GCM per project)
- Change detection via hash comparison
- Runtime loading via `ALIEN_SECRETS` env var

## Platform Implementations

### AWS (vault/data-read)

```json
{
  "grant": {
    "actions": ["secretsmanager:GetSecretValue", "secretsmanager:DescribeSecret"]
  },
  "binding": {
    "resource": {
      "resources": ["arn:aws:secretsmanager:${awsRegion}:${awsAccountId}:secret:${stackPrefix}-${resourceName}-*"]
    }
  }
}
```

### GCP (vault/data-read)

```json
{
  "grant": {
    "permissions": ["secretmanager.versions.access", "secretmanager.secrets.get"]
  },
  "binding": {
    "resource": {
      "scope": "projects/${projectName}",
      "condition": {
        "expression": "resource.name.startsWith('projects/${projectName}/secrets/${stackPrefix}-${resourceName}-')"
      }
    }
  }
}
```

### Azure (vault/data-read)

```json
{
  "grant": {
    "actions": ["Microsoft.KeyVault/vaults/secrets/getSecret/action", "Microsoft.KeyVault/vaults/secrets/read"]
  },
  "binding": {
    "resource": {
      "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.KeyVault/vaults/${stackPrefix}-${resourceName}"
    }
  }
}
```

## Lifecycle

Vaults are typically **frozen** resources—created once during InitialSetup and rarely changed:

```typescript
export default new alien.Stack("my-app")
  .add(userSecretsVault, "frozen")
  .add(func, "live")
  .build()
```

This makes sense because:
1. The vault is just a namespace (AWS/GCP) or a container (Azure)
2. Secrets inside the vault change; the vault itself doesn't
3. Permissions are applied during setup

