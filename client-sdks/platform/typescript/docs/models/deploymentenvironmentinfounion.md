# DeploymentEnvironmentInfoUnion

Cloud environment information


## Supported Types

### `models.DeploymentEnvironmentInfoAws`

```typescript
const value: models.DeploymentEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.DeploymentEnvironmentInfoGcp`

```typescript
const value: models.DeploymentEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.DeploymentEnvironmentInfoAzure`

```typescript
const value: models.DeploymentEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.DeploymentEnvironmentInfoLocal`

```typescript
const value: models.DeploymentEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "actual-metabolite.biz",
  os: "Windows Phone",
  platform: "local",
};
```

### `models.DeploymentEnvironmentInfoTest`

```typescript
const value: models.DeploymentEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

