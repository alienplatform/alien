# DataUnion13


## Supported Types

### `operations.DataAwsCodeBuild`

```typescript
const value: operations.DataAwsCodeBuild = {
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

### `operations.DataGcpCloudBuild`

```typescript
const value: operations.DataGcpCloudBuild = {
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

### `operations.DataAzureContainerApps2`

```typescript
const value: operations.DataAzureContainerApps2 = {
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

### `operations.DataKubernetesJob`

```typescript
const value: operations.DataKubernetesJob = {
  conditionCount: 902553,
  events: [],
  jobName: "<value>",
  namespace: "<value>",
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
    lifecycle: "scaling",
    partial: false,
    stale: true,
  },
  backend: "kubernetesJob",
};
```

