# DataUnion6


## Supported Types

### `operations.DataAwsSqs`

```typescript
const value: operations.DataAwsSqs = {
  approximateCounts: false,
  events: [],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "awsSqs",
};
```

### `operations.DataGcpPubSub`

```typescript
const value: operations.DataGcpPubSub = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-08-12T10:51:43.799Z"),
      severity: "warning",
    },
  ],
  messageStorageAllowedPersistenceRegions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  subscriptionLabels: {
    "key": "<value>",
  },
  subscriptionPushAttributes: {},
  topicLabels: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  topicName: "<value>",
  backend: "gcpPubSub",
};
```

### `operations.DataAzureServiceBus`

```typescript
const value: operations.DataAzureServiceBus = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-12-11T21:43:07.631Z"),
      severity: "error",
    },
  ],
  name: "<value>",
  namespaceName: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "azureServiceBus",
};
```

### `operations.DataLocal6`

```typescript
const value: operations.DataLocal6 = {
  events: [],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

