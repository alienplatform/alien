# DeploymentInfoManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.DeploymentInfoManagementConfigAws`

```typescript
const value: models.DeploymentInfoManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.DeploymentInfoManagementConfigGcp`

```typescript
const value: models.DeploymentInfoManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.DeploymentInfoManagementConfigAzure`

```typescript
const value: models.DeploymentInfoManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.DeploymentInfoManagementConfigKubernetes`

```typescript
const value: models.DeploymentInfoManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

