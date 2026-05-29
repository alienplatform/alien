# DataQueue

## Example Usage

```typescript
import { DataQueue } from "@alienplatform/platform-api/models/operations";

let value: DataQueue = {
  data: {
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
      "key1": "<value>",
      "key2": "<value>",
    },
    subscriptionPushAttributes: {},
    topicLabels: {
      "key": "<value>",
    },
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