# VaultHeartbeatData


## Supported Types

### `models.VaultHeartbeatDataAwsParameterStore`

```typescript
const value: models.VaultHeartbeatDataAwsParameterStore = {
  accountId: "<id>",
  parameterMetadataSampled: true,
  prefix: "<value>",
  region: "<value>",
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
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "awsParameterStore",
};
```

### `models.VaultHeartbeatDataGcpSecretManager`

```typescript
const value: models.VaultHeartbeatDataGcpSecretManager = {
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
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
    lifecycle: "scaling",
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
  name: "<value>",
  privateEndpointConnectionCount: 62989,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: false,
  secretMetadataListed: true,
  softDeleteEnabled: true,
  softDeleteRetentionDays: 839995,
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
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "azureKeyVault",
};
```

### `models.VaultHeartbeatDataKubernetesSecret`

```typescript
const value: models.VaultHeartbeatDataKubernetesSecret = {
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: true,
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
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "kubernetesSecret",
};
```

### `models.VaultHeartbeatDataLocal`

```typescript
const value: models.VaultHeartbeatDataLocal = {
  path: "/sys",
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
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

