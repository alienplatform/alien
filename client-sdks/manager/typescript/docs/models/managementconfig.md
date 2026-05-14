# ManagementConfig

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.ManagementConfigAws`

```typescript
const value: models.ManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.ManagementConfigGcp`

```typescript
const value: models.ManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.ManagementConfigAzure`

```typescript
const value: models.ManagementConfigAzure = {
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `models.ManagementConfigKubernetes`

```typescript
const value: models.ManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

