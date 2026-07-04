# DataUnion14


## Supported Types

### `operations.DataAwsCodeBuild`

```typescript
const value: operations.DataAwsCodeBuild = {
  encryptionKeyPresent: true,
  environmentVariableCount: 879452,
  projectName: "<value>",
  serviceRolePresent: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "awsCodeBuild",
};
```

### `operations.DataGcpCloudBuild`

```typescript
const value: operations.DataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 982514,
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "gcpCloudBuild",
};
```

### `operations.DataAzureContainerApps2`

```typescript
const value: operations.DataAzureContainerApps2 = {
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
    health: "healthy",
    lifecycle: "updating",
    partial: false,
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

