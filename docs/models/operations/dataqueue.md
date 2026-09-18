# DataQueue

## Example Usage

```typescript
import { DataQueue } from "@alienplatform/platform-api/models/operations";

let value: DataQueue = {
  data: {
    messageStorageAllowedPersistenceRegions: [
      "<value 1>",
      "<value 2>",
    ],
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "failed",
      partial: true,
      stale: true,
    },
    subscriptionLabels: {
      "key": "<value>",
    },
    subscriptionPushAttributes: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    topicLabels: {},
    topicName: "<value>",
    backend: "gcpPubSub",
  },
  resourceType: "queue",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion6* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"queue"*               | :heavy_check_mark:      | N/A                     |