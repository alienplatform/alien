# BuildHeartbeatData


## Supported Types

### `models.BuildHeartbeatDataAwsCodeBuild`

```typescript
const value: models.BuildHeartbeatDataAwsCodeBuild = {
  encryptionKeyPresent: false,
  environmentVariableCount: 168577,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  projectName: "<value>",
  serviceRolePresent: true,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
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
  events: [],
  location: "<value>",
  projectId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "gcpCloudBuild",
};
```

### `models.BuildHeartbeatDataAzureContainerApps`

```typescript
const value: models.BuildHeartbeatDataAzureContainerApps = {
  environmentVariableCount: 180128,
  events: [],
  managedEnvironmentId: "<id>",
  resourceGroupName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
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
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  jobName: "<value>",
  namespace: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "kubernetesJob",
};
```

