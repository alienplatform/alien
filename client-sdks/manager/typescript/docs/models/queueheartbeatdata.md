# QueueHeartbeatData


## Supported Types

### `models.QueueHeartbeatDataAwsSqs`

```typescript
const value: models.QueueHeartbeatDataAwsSqs = {
  approximateCounts: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "awsSqs",
};
```

### `models.QueueHeartbeatDataGcpPubSub`

```typescript
const value: models.QueueHeartbeatDataGcpPubSub = {
  events: [],
  messageStorageAllowedPersistenceRegions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  subscriptionLabels: {
    "key": "<value>",
  },
  subscriptionPushAttributes: {
    "key": "<value>",
    "key1": "<value>",
  },
  topicLabels: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  topicName: "<value>",
  backend: "gcpPubSub",
};
```

### `models.QueueHeartbeatDataAzureServiceBus`

```typescript
const value: models.QueueHeartbeatDataAzureServiceBus = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  namespaceName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "azureServiceBus",
};
```

### `models.QueueHeartbeatDataLocal`

```typescript
const value: models.QueueHeartbeatDataLocal = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

