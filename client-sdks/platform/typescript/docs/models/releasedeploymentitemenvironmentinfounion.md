# ReleaseDeploymentItemEnvironmentInfoUnion

Cloud environment information


## Supported Types

### `models.ReleaseDeploymentItemEnvironmentInfoAws`

```typescript
const value: models.ReleaseDeploymentItemEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.ReleaseDeploymentItemEnvironmentInfoGcp`

```typescript
const value: models.ReleaseDeploymentItemEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.ReleaseDeploymentItemEnvironmentInfoAzure`

```typescript
const value: models.ReleaseDeploymentItemEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.ReleaseDeploymentItemEnvironmentInfoLocal`

```typescript
const value: models.ReleaseDeploymentItemEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "unused-overcoat.biz",
  os: "Linux",
  platform: "local",
};
```

### `models.ReleaseDeploymentItemEnvironmentInfoTest`

```typescript
const value: models.ReleaseDeploymentItemEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

