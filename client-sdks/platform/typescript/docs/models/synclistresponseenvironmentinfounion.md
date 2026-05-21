# SyncListResponseEnvironmentInfoUnion

Cloud environment information


## Supported Types

### `models.SyncListResponseEnvironmentInfoAws`

```typescript
const value: models.SyncListResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

### `models.SyncListResponseEnvironmentInfoGcp`

```typescript
const value: models.SyncListResponseEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.SyncListResponseEnvironmentInfoAzure`

```typescript
const value: models.SyncListResponseEnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.SyncListResponseEnvironmentInfoLocal`

```typescript
const value: models.SyncListResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "snoopy-independence.com",
  os: "MacOS",
  platform: "local",
};
```

### `models.SyncListResponseEnvironmentInfoTest`

```typescript
const value: models.SyncListResponseEnvironmentInfoTest = {
  testId: "<id>",
  platform: "test",
};
```

### `any`

```typescript
const value: any = "<value>";
```

