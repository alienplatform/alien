# WorkerHeartbeatData


## Supported Types

### `models.WorkerHeartbeatDataAwsLambda`

```typescript
const value: models.WorkerHeartbeatDataAwsLambda = {
  functionName: "<value>",
  functionUrlCorsPresent: true,
  layerCount: 637035,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 109377,
  backend: "awsLambda",
};
```

### `models.WorkerHeartbeatDataGcpCloudRun`

```typescript
const value: models.WorkerHeartbeatDataGcpCloudRun = {
  service: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  trafficCount: 570634,
  urls: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  backend: "gcpCloudRun",
};
```

### `models.WorkerHeartbeatDataAzureContainerApps`

```typescript
const value: models.WorkerHeartbeatDataAzureContainerApps = {
  appName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "azureContainerApps",
};
```

### `models.WorkerHeartbeatDataKubernetes`

```typescript
const value: models.WorkerHeartbeatDataKubernetes = {
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  name: "<value>",
  namespace: "<value>",
  pods: [],
  replicas: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 374516,
  workloadKind: "daemonSet",
  backend: "kubernetes",
};
```

### `models.WorkerHeartbeatDataLocal`

```typescript
const value: models.WorkerHeartbeatDataLocal = {
  commandSupported: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      severity: "error",
      timestamp: new Date("2026-10-18T22:04:45.971Z"),
    },
  ],
  imagePathPresent: true,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 305200,
  backend: "local",
};
```

