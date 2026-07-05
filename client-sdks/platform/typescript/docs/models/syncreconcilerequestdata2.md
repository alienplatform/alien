# SyncReconcileRequestData2

## Example Usage

```typescript
import { SyncReconcileRequestData2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData2 = {
  managedTags: {
    "key": "<value>",
    "key1": "<value>",
  },
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `location`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `managedTags`                                                              | Record<string, *string*>                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `provisioningState`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceId`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus60](../models/resourceheartbeatstatus60.md) | :heavy_check_mark:                                                         | N/A                                                                        |