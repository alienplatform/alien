# SyncReconcileRequestEnvironmentInfoUnion


## Supported Types

### `models.SyncReconcileRequestEnvironmentInfoAws`

```typescript
const value: models.SyncReconcileRequestEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.SyncReconcileRequestEnvironmentInfoGcp`

```typescript
const value: models.SyncReconcileRequestEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.SyncReconcileRequestEnvironmentInfoAzure`

```typescript
const value: models.SyncReconcileRequestEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncReconcileRequestEnvironmentInfoLocal`

```typescript
const value: models.SyncReconcileRequestEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "recent-requirement.name",
  os: "Symbian",
  platform: "local",
};
```

### `models.SyncReconcileRequestEnvironmentInfoTest`

```typescript
const value: models.SyncReconcileRequestEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

