# BuildHeartbeatData


## Supported Types

### `models.BuildHeartbeatDataAwsCodeBuild`

```typescript
const value: models.BuildHeartbeatDataAwsCodeBuild = {
  encryptionKeyPresent: false,
  environmentVariableCount: 168577,
  projectName: "<value>",
  serviceRolePresent: false,
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
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "awsCodeBuild",
};
```

### `models.BuildHeartbeatDataGcpCloudBuild`

```typescript
const value: models.BuildHeartbeatDataGcpCloudBuild = {
  buildConfigId: "<id>",
  environmentVariableCount: 770056,
  location: "<value>",
  projectId: "<id>",
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
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "gcpCloudBuild",
};
```

### `models.BuildHeartbeatDataAzureContainerApps`

```typescript
const value: models.BuildHeartbeatDataAzureContainerApps = {
  environmentVariableCount: 180128,
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
    health: "degraded",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

### `models.BuildHeartbeatDataKubernetesJob`

```typescript
const value: models.BuildHeartbeatDataKubernetesJob = {
  conditionCount: 199682,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
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
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "kubernetesJob",
};
```

