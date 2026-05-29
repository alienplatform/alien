# ArtifactRegistryHeartbeatData


## Supported Types

### `models.ArtifactRegistryHeartbeatDataAwsEcr`

```typescript
const value: models.ArtifactRegistryHeartbeatDataAwsEcr = {
  events: [],
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://burdensome-best-seller.net",
  repositories: [],
  repositoriesTruncated: true,
  repositoryCount: 717377,
  repositoryPrefix: "<value>",
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
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "awsEcr",
};
```

### `models.ArtifactRegistryHeartbeatDataGcpArtifactRegistry`

```typescript
const value: models.ArtifactRegistryHeartbeatDataGcpArtifactRegistry = {
  cleanupPolicyCount: 419391,
  events: [],
  iamBindingCount: 807373,
  iamPolicyEtagPresent: false,
  iamRoles: [],
  kmsKeyNamePresent: true,
  labelCount: 648484,
  location: "<value>",
  projectId: "<id>",
  repositoryId: "<id>",
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
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "gcpArtifactRegistry",
};
```

### `models.ArtifactRegistryHeartbeatDataAzureContainerRegistry`

```typescript
const value: models.ArtifactRegistryHeartbeatDataAzureContainerRegistry = {
  adminUserEnabled: false,
  anonymousPullEnabled: true,
  dataEndpointHostNames: [],
  encryptionKeyIdentifierPresent: false,
  encryptionKeyVaultUriPresent: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  ipRuleCount: 61353,
  location: "<value>",
  managedTagCount: 809501,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: false,
  policyCount: 211805,
  privateEndpointConnectionCount: 509149,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
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
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  zoneRedundancy: "<value>",
  backend: "azureContainerRegistry",
};
```

### `models.ArtifactRegistryHeartbeatDataLocal`

```typescript
const value: models.ArtifactRegistryHeartbeatDataLocal = {
  events: [],
  reachable: false,
  registryUrl: "https://excited-armoire.net",
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
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

