# SyncReconcileRequestDataUnion2


## Supported Types

### `models.DataAwsLambda`

```typescript
const value: models.DataAwsLambda = {
  functionName: "<value>",
  functionUrlCorsPresent: false,
  layerCount: 515631,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "failed",
    partial: true,
    stale: false,
  },
  triggerCount: 414207,
  backend: "awsLambda",
};
```

### `models.DataGcpCloudRun`

```typescript
const value: models.DataGcpCloudRun = {
  service: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  trafficCount: 9050,
  urls: [
    "<value 1>",
  ],
  backend: "gcpCloudRun",
};
```

### `models.DataAzureContainerApps1`

```typescript
const value: models.DataAzureContainerApps1 = {
  appName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "azureContainerApps",
};
```

### `models.DataKubernetes1`

```typescript
const value: models.DataKubernetes1 = {
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
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  triggerCount: 303382,
  workloadKind: "daemonSet",
  backend: "kubernetes",
};
```

### `models.DataLocal2`

```typescript
const value: models.DataLocal2 = {
  commandSupported: true,
  events: [],
  imagePathPresent: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  triggerCount: 963366,
  backend: "local",
};
```

