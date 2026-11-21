# DataQueue

## Example Usage

```typescript
import { DataQueue } from "@alienplatform/platform-api/models";

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

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `data`                                                     | *models.SyncReconcileRequestDataUnion6*                    | :heavy_check_mark:                                         | N/A                                                        |
| `resourceType`                                             | [models.ResourceTypeQueue](../models/resourcetypequeue.md) | :heavy_check_mark:                                         | N/A                                                        |