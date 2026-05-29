# SyncReconcileRequestDataUnion12


## Supported Types

### `models.DataAwsEcr`

```typescript
const value: models.DataAwsEcr = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-07-20T04:34:55.254Z"),
      severity: "info",
    },
  ],
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://shiny-shadowbox.info/",
  repositories: [
    {
      createdAt: 3232.31,
      kmsKeyPresent: true,
      registryId: "<id>",
      repositoryArn: "<value>",
      repositoryName: "<value>",
      repositoryUri: "https://back-wear.com/",
    },
  ],
  repositoriesTruncated: true,
  repositoryCount: 601812,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "awsEcr",
};
```

### `models.DataGcpArtifactRegistry`

```typescript
const value: models.DataGcpArtifactRegistry = {
  cleanupPolicyCount: 150101,
  events: [],
  iamBindingCount: 412960,
  iamPolicyEtagPresent: true,
  iamRoles: [],
  kmsKeyNamePresent: false,
  labelCount: 394471,
  location: "<value>",
  projectId: "<id>",
  repositoryId: "<id>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "gcpArtifactRegistry",
};
```

### `models.DataAzureContainerRegistry`

```typescript
const value: models.DataAzureContainerRegistry = {
  adminUserEnabled: false,
  anonymousPullEnabled: true,
  dataEndpointHostNames: [
    "<value 1>",
    "<value 2>",
  ],
  encryptionKeyIdentifierPresent: false,
  encryptionKeyVaultUriPresent: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-03-09T17:11:17.646Z"),
      severity: "info",
    },
  ],
  ipRuleCount: 284832,
  location: "<value>",
  managedTagCount: 849232,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: true,
  policyCount: 875905,
  privateEndpointConnectionCount: 997002,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  zoneRedundancy: "<value>",
  backend: "azureContainerRegistry",
};
```

### `models.DataLocal10`

```typescript
const value: models.DataLocal10 = {
  events: [],
  reachable: true,
  registryUrl: "https://fearless-exhaust.biz",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

