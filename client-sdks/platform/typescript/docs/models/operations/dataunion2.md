# DataUnion2


## Supported Types

### `operations.DataAwsLambda`

```typescript
const value: operations.DataAwsLambda = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-07-19T03:10:06.736Z"),
      severity: "warning",
    },
  ],
  functionName: "<value>",
  functionUrlCorsPresent: false,
  layerCount: 965000,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: false,
    stale: false,
  },
  triggerCount: 857435,
  backend: "awsLambda",
};
```

### `operations.DataGcpCloudRun`

```typescript
const value: operations.DataGcpCloudRun = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-03-08T10:07:05.119Z"),
      severity: "warning",
    },
  ],
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
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  trafficCount: 335156,
  urls: [
    "<value 1>",
  ],
  backend: "gcpCloudRun",
};
```

### `operations.DataAzureContainerApps1`

```typescript
const value: operations.DataAzureContainerApps1 = {
  appName: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-05-02T10:27:21.624Z"),
      severity: "warning",
    },
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "updating",
    partial: false,
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
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-06-15T13:58:09.990Z"),
      severity: "info",
    },
  ],
  instances: [
    {
      name: "<value>",
      ownerReferences: [
        {
          controller: false,
          kind: "<value>",
          name: "<value>",
          uid: "<id>",
        },
      ],
      ready: true,
      restartCount: 303382,
    },
  ],
  name: "<value>",
  namespace: "<value>",
  replicas: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  triggerCount: 638946,
  workloadKind: "replicaSet",
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

