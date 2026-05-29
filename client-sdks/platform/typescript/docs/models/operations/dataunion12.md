# DataUnion12


## Supported Types

### `operations.DataAwsEcr`

```typescript
const value: operations.DataAwsEcr = {
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://mindless-bench.com",
  repositories: [
    {
      createdAt: 8040.15,
      kmsKeyPresent: true,
      registryId: "<id>",
      repositoryArn: "<value>",
      repositoryName: "<value>",
      repositoryUri: "https://glossy-disclosure.info",
    },
  ],
  repositoriesTruncated: true,
  repositoryCount: 980949,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
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
  iamBindingCount: 410292,
  iamPolicyEtagPresent: true,
  iamRoles: [],
  kmsKeyNamePresent: true,
  labelCount: 560331,
  location: "<value>",
  projectId: "<id>",
  repositoryId: "<id>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopped",
    partial: false,
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
  ipRuleCount: 806238,
  location: "<value>",
  managedTagCount: 395726,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: true,
  policyCount: 284832,
  privateEndpointConnectionCount: 849232,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "failed",
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
  reachable: true,
  registryUrl: "https://white-doorpost.biz/",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

