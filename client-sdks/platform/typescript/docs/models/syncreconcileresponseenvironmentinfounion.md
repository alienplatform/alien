# SyncReconcileResponseEnvironmentInfoUnion


## Supported Types

### `models.SyncReconcileResponseEnvironmentInfoAws`

```typescript
const value: models.SyncReconcileResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.SyncReconcileResponseEnvironmentInfoGcp`

```typescript
const value: models.SyncReconcileResponseEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.SyncReconcileResponseEnvironmentInfoAzure`

```typescript
const value: models.SyncReconcileResponseEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncReconcileResponseEnvironmentInfoLocal`

```typescript
const value: models.SyncReconcileResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "miserable-eyeliner.com",
  os: "Chrome OS",
  platform: "local",
};
```

### `models.SyncReconcileResponseEnvironmentInfoTest`

```typescript
const value: models.SyncReconcileResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

