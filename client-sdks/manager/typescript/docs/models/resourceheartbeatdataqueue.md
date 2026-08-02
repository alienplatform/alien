# ResourceHeartbeatDataQueue

## Example Usage

```typescript
import { ResourceHeartbeatDataQueue } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataQueue = {
  data: {
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
    subscriptionPushAttributes: {},
    topicLabels: {
      "key": "<value>",
      "key1": "<value>",
    },
    topicName: "<value>",
    backend: "gcpPubSub",
  },
  resourceType: "queue",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `data`                      | *models.QueueHeartbeatData* | :heavy_check_mark:          | N/A                         |
| `resourceType`              | *"queue"*                   | :heavy_check_mark:          | N/A                         |