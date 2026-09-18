# DeploymentListItemResponseEnvironmentInfoUnion

Cloud environment information


## Supported Types

### `models.DeploymentListItemResponseEnvironmentInfoAws`

```typescript
const value: models.DeploymentListItemResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.DeploymentListItemResponseEnvironmentInfoGcp`

```typescript
const value: models.DeploymentListItemResponseEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.DeploymentListItemResponseEnvironmentInfoAzure`

```typescript
const value: models.DeploymentListItemResponseEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.DeploymentListItemResponseEnvironmentInfoLocal`

```typescript
const value: models.DeploymentListItemResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "oily-object.com",
  os: "Symbian",
  platform: "local",
};
```

### `models.DeploymentListItemResponseEnvironmentInfoTest`

```typescript
const value: models.DeploymentListItemResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

