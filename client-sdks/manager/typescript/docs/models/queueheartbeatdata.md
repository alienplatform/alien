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
    lifecycle: "deleted",
    partial: true,
    stale: false,
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
    lifecycle: "deleted",
    partial: true,
    stale: false,
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
    lifecycle: "deleted",
    partial: true,
    stale: false,
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
    lifecycle: "deleted",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

