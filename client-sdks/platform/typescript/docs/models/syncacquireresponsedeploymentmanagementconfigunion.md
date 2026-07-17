# SyncAcquireResponseDeploymentManagementConfigUnion


## Supported Types

### `models.SyncAcquireResponseDeploymentManagementConfigAws`

```typescript
const value: models.SyncAcquireResponseDeploymentManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.SyncAcquireResponseDeploymentManagementConfigGcp`

```typescript
const value: models.SyncAcquireResponseDeploymentManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.SyncAcquireResponseDeploymentManagementConfigAzure`

```typescript
const value: models.SyncAcquireResponseDeploymentManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.SyncAcquireResponseDeploymentManagementConfigKubernetes`

```typescript
const value: models.SyncAcquireResponseDeploymentManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

