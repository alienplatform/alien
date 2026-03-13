# GetManagerManagementConfigResponse

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Agent Manager's ServiceAccount.


## Supported Types

### `operations.GetManagerManagementConfigAws`

```typescript
const value: operations.GetManagerManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `operations.Gcp`

```typescript
const value: operations.Gcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `operations.Azure`

```typescript
const value: operations.Azure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `operations.Kubernetes`

```typescript
const value: operations.Kubernetes = {
  platform: "kubernetes",
};
```

