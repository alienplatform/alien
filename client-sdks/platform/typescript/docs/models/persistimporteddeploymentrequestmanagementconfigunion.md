# PersistImportedDeploymentRequestManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.PersistImportedDeploymentRequestManagementConfigAws`

```typescript
const value: models.PersistImportedDeploymentRequestManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.PersistImportedDeploymentRequestManagementConfigGcp`

```typescript
const value: models.PersistImportedDeploymentRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.PersistImportedDeploymentRequestManagementConfigAzure`

```typescript
const value: models.PersistImportedDeploymentRequestManagementConfigAzure = {
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `models.PersistImportedDeploymentRequestManagementConfigKubernetes`

```typescript
const value: models.PersistImportedDeploymentRequestManagementConfigKubernetes =
  {
    platform: "kubernetes",
  };
```

