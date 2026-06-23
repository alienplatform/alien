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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: false,
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
| `status`                                                                   | [models.ResourceHeartbeatStatus56](../models/resourceheartbeatstatus56.md) | :heavy_check_mark:                                                         | N/A                                                                        |