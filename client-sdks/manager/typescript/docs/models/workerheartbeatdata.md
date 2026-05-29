# WorkerHeartbeatData


## Supported Types

### `models.WorkerHeartbeatDataAwsLambda`

```typescript
const value: models.WorkerHeartbeatDataAwsLambda = {
  events: [],
  functionName: "<value>",
  functionUrlCorsPresent: false,
  layerCount: 109377,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 947900,
  backend: "awsLambda",
};
```

### `models.WorkerHeartbeatDataGcpCloudRun`

```typescript
const value: models.WorkerHeartbeatDataGcpCloudRun = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  service: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  trafficCount: 874910,
  urls: [
    "<value 1>",
    "<value 2>",
  ],
  backend: "gcpCloudRun",
};
```

### `models.WorkerHeartbeatDataAzureContainerApps`

```typescript
const value: models.WorkerHeartbeatDataAzureContainerApps = {
  appName: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
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
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  instances: [],
  name: "<value>",
  namespace: "<value>",
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
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  imagePathPresent: false,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 932409,
  backend: "local",
};
```

