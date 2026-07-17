# SyncAcquireResponseDeploymentEnvironmentInfoUnion


## Supported Types

### `models.SyncAcquireResponseDeploymentEnvironmentInfoAws`

```typescript
const value: models.SyncAcquireResponseDeploymentEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.SyncAcquireResponseDeploymentEnvironmentInfoGcp`

```typescript
const value: models.SyncAcquireResponseDeploymentEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.SyncAcquireResponseDeploymentEnvironmentInfoAzure`

```typescript
const value: models.SyncAcquireResponseDeploymentEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncAcquireResponseDeploymentEnvironmentInfoLocal`

```typescript
const value: models.SyncAcquireResponseDeploymentEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "leading-graffiti.com",
  os: "Linux",
  platform: "local",
};
```

### `models.SyncAcquireResponseDeploymentEnvironmentInfoTest`

```typescript
const value: models.SyncAcquireResponseDeploymentEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

