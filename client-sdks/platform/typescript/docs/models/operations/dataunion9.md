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

### `operations.DataAzureKeyVault`

```typescript
const value: operations.DataAzureKeyVault = {
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
        reason: "api-unavailable",
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
        reason: "not-installed",
        severity: "info",
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
