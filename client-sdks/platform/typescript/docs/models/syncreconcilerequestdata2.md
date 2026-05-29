# SyncReconcileRequestData2

## Example Usage

```typescript
import { SyncReconcileRequestData2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData2 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-03-07T19:36:50.644Z"),
      severity: "error",
    },
  ],
  managedTags: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent56](../models/syncreconcilerequestevent56.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `location`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `managedTags`                                                                    | Record<string, *string*>                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `provisioningState`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus56](../models/heartbeatstatus56.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |