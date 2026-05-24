# CloudFormationCallbackRequestManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.CloudFormationCallbackRequestManagementConfigAws`

```typescript
const value: models.CloudFormationCallbackRequestManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.CloudFormationCallbackRequestManagementConfigGcp`

```typescript
const value: models.CloudFormationCallbackRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.CloudFormationCallbackRequestManagementConfigAzure`

```typescript
const value: models.CloudFormationCallbackRequestManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.CloudFormationCallbackRequestManagementConfigKubernetes`

```typescript
const value: models.CloudFormationCallbackRequestManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

