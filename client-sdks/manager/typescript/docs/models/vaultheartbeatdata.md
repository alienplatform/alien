# VaultHeartbeatData


## Supported Types

### `models.VaultHeartbeatDataAwsParameterStore`

```typescript
const value: models.VaultHeartbeatDataAwsParameterStore = {
  accountId: "<id>",
  events: [],
  parameterMetadataSampled: false,
  prefix: "<value>",
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "awsParameterStore",
};
```

### `models.VaultHeartbeatDataGcpSecretManager`

```typescript
const value: models.VaultHeartbeatDataGcpSecretManager = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "gcpSecretManager",
};
```

### `models.VaultHeartbeatDataAzureKeyVault`

```typescript
const value: models.VaultHeartbeatDataAzureKeyVault = {
  accessPolicyCount: 590752,
  events: [],
  name: "<value>",
  privateEndpointConnectionCount: 929405,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: true,
  secretMetadataListed: true,
  softDeleteEnabled: false,
  softDeleteRetentionDays: 926964,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "azureKeyVault",
};
```

### `models.VaultHeartbeatDataKubernetesSecret`

```typescript
const value: models.VaultHeartbeatDataKubernetesSecret = {
  events: [],
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "kubernetesSecret",
};
```

### `models.VaultHeartbeatDataLocal`

```typescript
const value: models.VaultHeartbeatDataLocal = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  path: "/var/tmp",
  pathExists: false,
  secretMetadataListed: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

