# SyncListResponseManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.SyncListResponseManagementConfigAws`

```typescript
const value: models.SyncListResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.SyncListResponseManagementConfigGcp`

```typescript
const value: models.SyncListResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.SyncListResponseManagementConfigAzure`

```typescript
const value: models.SyncListResponseManagementConfigAzure = {
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncListResponseManagementConfigKubernetes`

```typescript
const value: models.SyncListResponseManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

