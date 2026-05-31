# ArtifactRegistryHeartbeatData


## Supported Types

### `models.ArtifactRegistryHeartbeatDataAwsEcr`

```typescript
const value: models.ArtifactRegistryHeartbeatDataAwsEcr = {
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://unlined-bowler.com/",
  repositories: [
    {
      createdAt: 781.7,
      kmsKeyPresent: true,
      registryId: "<id>",
      repositoryArn: "<value>",
      repositoryName: "<value>",
      repositoryUri: "https://wavy-eggplant.org",
    },
  ],
  repositoriesTruncated: true,
  repositoryCount: 952265,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "awsEcr",
};
```

### `models.ArtifactRegistryHeartbeatDataGcpArtifactRegistry`

```typescript
const value: models.ArtifactRegistryHeartbeatDataGcpArtifactRegistry = {
  cleanupPolicyCount: 419391,
  iamBindingCount: 128533,
  iamPolicyEtagPresent: false,
  iamRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  kmsKeyNamePresent: true,
  labelCount: 439989,
  location: "<value>",
  projectId: "<id>",
  repositoryId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
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
  ipRuleCount: 615073,
  location: "<value>",
  managedTagCount: 61353,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: false,
  policyCount: 966370,
  privateEndpointConnectionCount: 211805,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  zoneRedundancy: "<value>",
  backend: "azureContainerRegistry",
};
```

### `models.ArtifactRegistryHeartbeatDataLocal`

```typescript
const value: models.ArtifactRegistryHeartbeatDataLocal = {
  reachable: true,
  registryUrl: "https://which-devastation.com",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

