# SyncReconcileRequestDataUnion10


## Supported Types

### `models.DataAwsParameterStore`

```typescript
const value: models.DataAwsParameterStore = {
  accountId: "<id>",
  parameterMetadataSampled: true,
  prefix: "<value>",
  region: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "awsParameterStore",
};
```

### `models.DataGcpSecretManager`

```typescript
const value: models.DataGcpSecretManager = {
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
  backend: "gcpSecretManager",
};
```

### `models.DataAzureKeyVault1`

```typescript
const value: models.DataAzureKeyVault1 = {
  accessPolicyCount: 582917,
  name: "<value>",
  privateEndpointConnectionCount: 112234,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: true,
  secretMetadataListed: false,
  softDeleteEnabled: true,
  softDeleteRetentionDays: 457442,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "azureKeyVault",
};
```

### `models.DataKubernetesSecret`

```typescript
const value: models.DataKubernetesSecret = {
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "kubernetesSecret",
};
```

### `models.DataLocal10`

```typescript
const value: models.DataLocal10 = {
  path: "/home/user",
  pathExists: true,
  secretMetadataListed: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```
