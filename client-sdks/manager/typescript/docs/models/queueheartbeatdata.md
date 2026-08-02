# QueueHeartbeatData


## Supported Types

### `models.QueueHeartbeatDataAwsSqs`

```typescript
const value: models.QueueHeartbeatDataAwsSqs = {
  approximateCounts: false,
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "awsSqs",
};
```

### `models.QueueHeartbeatDataGcpPubSub`

```typescript
const value: models.QueueHeartbeatDataGcpPubSub = {
  messageStorageAllowedPersistenceRegions: [
    "<value 1>",
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  subscriptionLabels: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  subscriptionPushAttributes: {
    "key": "<value>",
  },
  topicLabels: {
    "key": "<value>",
    "key1": "<value>",
  },
  topicName: "<value>",
  backend: "gcpPubSub",
};
```

### `models.QueueHeartbeatDataAzureServiceBus`

```typescript
const value: models.QueueHeartbeatDataAzureServiceBus = {
  name: "<value>",
  namespaceName: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "azureServiceBus",
};
```

### `models.QueueHeartbeatDataLocal`

```typescript
const value: models.QueueHeartbeatDataLocal = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

