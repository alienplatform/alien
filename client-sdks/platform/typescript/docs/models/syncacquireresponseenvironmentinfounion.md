# SyncAcquireResponseEnvironmentInfoUnion


## Supported Types

### `models.SyncAcquireResponseEnvironmentInfoAws`

```typescript
const value: models.SyncAcquireResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.SyncAcquireResponseEnvironmentInfoGcp`

```typescript
const value: models.SyncAcquireResponseEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.SyncAcquireResponseEnvironmentInfoAzure`

```typescript
const value: models.SyncAcquireResponseEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncAcquireResponseEnvironmentInfoLocal`

```typescript
const value: models.SyncAcquireResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "distorted-provider.info",
  os: "Blackberry",
  platform: "local",
};
```

### `models.SyncAcquireResponseEnvironmentInfoTest`

```typescript
const value: models.SyncAcquireResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

