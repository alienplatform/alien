# DebugSessionDeploymentEnvironmentInfoUnion

Platform-specific environment information


## Supported Types

### `models.DebugSessionDeploymentEnvironmentInfoAws`

```typescript
const value: models.DebugSessionDeploymentEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.DebugSessionDeploymentEnvironmentInfoGcp`

```typescript
const value: models.DebugSessionDeploymentEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.DebugSessionDeploymentEnvironmentInfoAzure`

```typescript
const value: models.DebugSessionDeploymentEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.DebugSessionDeploymentEnvironmentInfoLocal`

```typescript
const value: models.DebugSessionDeploymentEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "whimsical-offset.net",
  os: "Chrome OS",
  platform: "local",
};
```

### `models.DebugSessionDeploymentEnvironmentInfoTest`

```typescript
const value: models.DebugSessionDeploymentEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```
