# EnvironmentInfo

Platform-specific environment information


## Supported Types

### `models.EnvironmentInfoAws`

```typescript
const value: models.EnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.EnvironmentInfoGcp`

```typescript
const value: models.EnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.EnvironmentInfoAzure`

```typescript
const value: models.EnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.EnvironmentInfoLocal`

```typescript
const value: models.EnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "pertinent-spear.com",
  os: "Windows Phone",
  platform: "local",
};
```

### `models.EnvironmentInfoTest`

```typescript
const value: models.EnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

