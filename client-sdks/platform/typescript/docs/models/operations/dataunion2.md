# DataUnion2


## Supported Types

### `operations.DataAwsLambda`

```typescript
const value: operations.DataAwsLambda = {
  functionName: "<value>",
  functionUrlCorsPresent: false,
  layerCount: 515631,
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
    lifecycle: "failed",
    partial: true,
    stale: false,
  },
  triggerCount: 414207,
  backend: "awsLambda",
};
```

### `operations.DataGcpCloudRun`

```typescript
const value: operations.DataGcpCloudRun = {
  service: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  trafficCount: 12255,
  urls: [
    "<value 1>",
    "<value 2>",
  ],
  backend: "gcpCloudRun",
};
```

### `operations.DataAzureContainerApps1`

```typescript
const value: operations.DataAzureContainerApps1 = {
  appName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

### `operations.DataKubernetes1`

```typescript
const value: operations.DataKubernetes1 = {
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

### `operations.DataLocal2`

```typescript
const value: operations.DataLocal2 = {
  commandSupported: true,
  events: [],
  imagePathPresent: false,
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
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  triggerCount: 963366,
  backend: "local",
};
```

