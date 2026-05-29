# ResourceHeartbeatDataQueue

## Example Usage

```typescript
import { ResourceHeartbeatDataQueue } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataQueue = {
  data: {
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
    subscriptionLabels: {},
    subscriptionPushAttributes: {
      "key": "<value>",
      "key1": "<value>",
    },
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