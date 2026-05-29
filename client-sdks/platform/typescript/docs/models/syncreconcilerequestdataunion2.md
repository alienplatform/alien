# SyncReconcileRequestDataUnion2


## Supported Types

### `models.DataAwsLambda`

```typescript
const value: models.DataAwsLambda = {
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

### `models.DataGcpCloudRun`

```typescript
const value: models.DataGcpCloudRun = {
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
        reason: "not-installed",
        severity: "info",
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

### `models.DataAzureContainerApps1`

```typescript
const value: models.DataAzureContainerApps1 = {
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

### `models.DataKubernetes1`

```typescript
const value: models.DataKubernetes1 = {
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
        reason: "api-unavailable",
        severity: "error",
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

