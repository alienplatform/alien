# DeploymentStateEnvironmentInfoUnion


## Supported Types

### `models.DeploymentStateEnvironmentInfoAws`

```typescript
const value: models.DeploymentStateEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.DeploymentStateEnvironmentInfoGcp`

```typescript
const value: models.DeploymentStateEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.DeploymentStateEnvironmentInfoAzure`

```typescript
const value: models.DeploymentStateEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.DeploymentStateEnvironmentInfoLocal`

```typescript
const value: models.DeploymentStateEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "warlike-expense.org",
  os: "MacOS",
  platform: "local",
};
```

### `models.DeploymentStateEnvironmentInfoTest`

```typescript
const value: models.DeploymentStateEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

