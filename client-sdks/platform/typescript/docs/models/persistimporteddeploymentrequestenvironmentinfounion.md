# PersistImportedDeploymentRequestEnvironmentInfoUnion

Platform-specific environment information


## Supported Types

### `models.PersistImportedDeploymentRequestEnvironmentInfoAws`

```typescript
const value: models.PersistImportedDeploymentRequestEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.PersistImportedDeploymentRequestEnvironmentInfoGcp`

```typescript
const value: models.PersistImportedDeploymentRequestEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.PersistImportedDeploymentRequestEnvironmentInfoAzure`

```typescript
const value: models.PersistImportedDeploymentRequestEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.PersistImportedDeploymentRequestEnvironmentInfoLocal`

```typescript
const value: models.PersistImportedDeploymentRequestEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "well-off-maintainer.net",
  os: "BeOS",
  platform: "local",
};
```

### `models.PersistImportedDeploymentRequestEnvironmentInfoTest`

```typescript
const value: models.PersistImportedDeploymentRequestEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

