# NewDeploymentRequestEnvironmentInfoUnion

Cloud environment information


## Supported Types

### `models.NewDeploymentRequestEnvironmentInfoAws`

```typescript
const value: models.NewDeploymentRequestEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.NewDeploymentRequestEnvironmentInfoGcp`

```typescript
const value: models.NewDeploymentRequestEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.NewDeploymentRequestEnvironmentInfoAzure`

```typescript
const value: models.NewDeploymentRequestEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.NewDeploymentRequestEnvironmentInfoLocal`

```typescript
const value: models.NewDeploymentRequestEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "legal-scale.net",
  os: "Windows Phone",
  platform: "local",
};
```

### `models.NewDeploymentRequestEnvironmentInfoTest`

```typescript
const value: models.NewDeploymentRequestEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

