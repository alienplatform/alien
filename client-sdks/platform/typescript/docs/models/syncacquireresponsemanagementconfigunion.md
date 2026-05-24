# SyncAcquireResponseManagementConfigUnion


## Supported Types

### `models.SyncAcquireResponseManagementConfigAws`

```typescript
const value: models.SyncAcquireResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.SyncAcquireResponseManagementConfigGcp`

```typescript
const value: models.SyncAcquireResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.SyncAcquireResponseManagementConfigAzure`

```typescript
const value: models.SyncAcquireResponseManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.SyncAcquireResponseManagementConfigKubernetes`

```typescript
const value: models.SyncAcquireResponseManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

