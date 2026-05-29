# DataUnion12


## Supported Types

### `operations.DataAwsEcr`

```typescript
const value: operations.DataAwsEcr = {
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
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "awsEcr",
};
```

### `operations.DataGcpArtifactRegistry`

```typescript
const value: operations.DataGcpArtifactRegistry = {
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
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "gcpArtifactRegistry",
};
```

### `operations.DataAzureContainerRegistry`

```typescript
const value: operations.DataAzureContainerRegistry = {
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

### `operations.DataLocal10`

```typescript
const value: operations.DataLocal10 = {
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

