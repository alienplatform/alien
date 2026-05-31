# ImportSourceManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.ImportSourceManagementConfigAws`

```typescript
const value: models.ImportSourceManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.ImportSourceManagementConfigGcp`

```typescript
const value: models.ImportSourceManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.ImportSourceManagementConfigAzure`

```typescript
const value: models.ImportSourceManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.ImportSourceManagementConfigKubernetes`

```typescript
const value: models.ImportSourceManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

