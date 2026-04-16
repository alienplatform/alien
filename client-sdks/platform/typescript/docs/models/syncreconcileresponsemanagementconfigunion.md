# SyncReconcileResponseManagementConfigUnion


## Supported Types

### `models.SyncReconcileResponseManagementConfigAws`

```typescript
const value: models.SyncReconcileResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.SyncReconcileResponseManagementConfigGcp`

```typescript
const value: models.SyncReconcileResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.SyncReconcileResponseManagementConfigAzure`

```typescript
const value: models.SyncReconcileResponseManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncReconcileResponseManagementConfigKubernetes`

```typescript
const value: models.SyncReconcileResponseManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

