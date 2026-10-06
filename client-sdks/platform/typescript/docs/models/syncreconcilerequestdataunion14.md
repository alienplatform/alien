# SyncReconcileRequestDataUnion14


## Supported Types

### `models.DataAwsCodeBuild`

```typescript
const value: models.DataAwsCodeBuild = {
  encryptionKeyPresent: true,
  environmentVariableCount: 879452,
  projectName: "<value>",
  serviceRolePresent: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  backend: "awsCodeBuild",
};
```

### `models.DataGcpCloudBuild`

```typescript
const value: models.DataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 982514,
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "scaling",
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
  managedEnvironmentId: "<id>",
  resourceGroupName: "<value>",
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
    lifecycle: "deleting",
    partial: true,
    stale: true,
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
        reason: "forbidden",
        severity: "warning",
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

