# SyncReconcileRequestDataUnion9


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

### `models.DataAzureKeyVault`

```typescript
const value: models.DataAzureKeyVault = {
  accessPolicyCount: 923246,
  name: "<value>",
  privateEndpointConnectionCount: 319306,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: true,
  secretMetadataListed: false,
  softDeleteEnabled: false,
  softDeleteRetentionDays: 497787,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
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
        reason: "collection-failed",
        severity: "info",
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

### `models.DataLocal9`

```typescript
const value: models.DataLocal9 = {
  path: "/usr/include",
  pathExists: false,
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```
