# CommandDeploymentInfoEnvironmentInfoUnion

Platform-specific environment information


## Supported Types

### `models.CommandDeploymentInfoEnvironmentInfoAws`

```typescript
const value: models.CommandDeploymentInfoEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.CommandDeploymentInfoEnvironmentInfoGcp`

```typescript
const value: models.CommandDeploymentInfoEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.CommandDeploymentInfoEnvironmentInfoAzure`

```typescript
const value: models.CommandDeploymentInfoEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.CommandDeploymentInfoEnvironmentInfoLocal`

```typescript
const value: models.CommandDeploymentInfoEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "idealistic-disposer.info",
  os: "iOS",
  platform: "local",
};
```

### `models.CommandDeploymentInfoEnvironmentInfoTest`

```typescript
const value: models.CommandDeploymentInfoEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

