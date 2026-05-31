# ResolveResponseManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.ResolveResponseManagementConfigAws`

```typescript
const value: models.ResolveResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.ResolveResponseManagementConfigGcp`

```typescript
const value: models.ResolveResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.ResolveResponseManagementConfigAzure`

```typescript
const value: models.ResolveResponseManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.ResolveResponseManagementConfigKubernetes`

```typescript
const value: models.ResolveResponseManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

