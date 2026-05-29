# SyncReconcileRequestDataUnion8


## Supported Types

### `models.DataAwsParameterStore`

```typescript
const value: models.DataAwsParameterStore = {
  accountId: "<id>",
  events: [],
  parameterMetadataSampled: true,
  prefix: "<value>",
  region: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "awsParameterStore",
};
```

### `models.DataGcpSecretManager`

```typescript
const value: models.DataGcpSecretManager = {
  events: [],
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "updating",
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
  events: [],
  name: "<value>",
  privateEndpointConnectionCount: 363497,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: false,
  secretMetadataListed: false,
  softDeleteEnabled: true,
  softDeleteRetentionDays: 24015,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "azureKeyVault",
};
```

### `models.DataKubernetesSecret`

```typescript
const value: models.DataKubernetesSecret = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-12-05T01:04:58.637Z"),
      severity: "info",
    },
  ],
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "kubernetesSecret",
};
```

### `models.DataLocal8`

```typescript
const value: models.DataLocal8 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-07-08T01:45:59.103Z"),
      severity: "error",
    },
  ],
  path: "/usr/local/src",
  pathExists: true,
  secretMetadataListed: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

