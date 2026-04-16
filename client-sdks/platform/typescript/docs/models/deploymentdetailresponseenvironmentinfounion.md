# DeploymentDetailResponseEnvironmentInfoUnion

Cloud environment information


## Supported Types

### `models.DeploymentDetailResponseEnvironmentInfoAws`

```typescript
const value: models.DeploymentDetailResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.DeploymentDetailResponseEnvironmentInfoGcp`

```typescript
const value: models.DeploymentDetailResponseEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.DeploymentDetailResponseEnvironmentInfoAzure`

```typescript
const value: models.DeploymentDetailResponseEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.DeploymentDetailResponseEnvironmentInfoLocal`

```typescript
const value: models.DeploymentDetailResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "official-wear.biz",
  os: "BeOS",
  platform: "local",
};
```

### `models.DeploymentDetailResponseEnvironmentInfoTest`

```typescript
const value: models.DeploymentDetailResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

