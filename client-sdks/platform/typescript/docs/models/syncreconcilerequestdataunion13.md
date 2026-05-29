# SyncReconcileRequestDataUnion13


## Supported Types

### `models.DataAwsCodeBuild`

```typescript
const value: models.DataAwsCodeBuild = {
  encryptionKeyPresent: true,
  environmentVariableCount: 879452,
  events: [],
  projectName: "<value>",
  serviceRolePresent: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "awsCodeBuild",
};
```

### `models.DataGcpCloudBuild`

```typescript
const value: models.DataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 982514,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-11-14T23:58:06.955Z"),
      severity: "warning",
    },
  ],
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "gcpCloudBuild",
};
```

### `models.DataAzureContainerApps2`

```typescript
const value: models.DataAzureContainerApps2 = {
  environmentVariableCount: 246098,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-12-26T22:26:53.665Z"),
      severity: "info",
    },
  ],
  managedEnvironmentId: "<id>",
  resourceGroupName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "failed",
    partial: true,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

### `models.DataKubernetesJob`

```typescript
const value: models.DataKubernetesJob = {
  conditionCount: 902553,
  events: [],
  jobName: "<value>",
  namespace: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "scaling",
    partial: false,
    stale: true,
  },
  backend: "kubernetesJob",
};
```

