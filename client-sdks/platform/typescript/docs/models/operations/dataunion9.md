# DataUnion9


## Supported Types

### `operations.DataAwsParameterStore`

```typescript
const value: operations.DataAwsParameterStore = {
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

### `operations.DataGcpSecretManager`

```typescript
const value: operations.DataGcpSecretManager = {
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

### `operations.DataAzureKeyVault1`

```typescript
const value: operations.DataAzureKeyVault1 = {
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
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "azureKeyVault",
};
```

### `operations.DataKubernetesSecret`

```typescript
const value: operations.DataKubernetesSecret = {
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "kubernetesSecret",
};
```

### `operations.DataLocal9`

```typescript
const value: operations.DataLocal9 = {
  path: "/usr/include",
  pathExists: false,
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

