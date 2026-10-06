# ResourceHeartbeatStatus15

## Example Usage

```typescript
import { ResourceHeartbeatStatus15 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus15 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "updating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue15](../models/syncreconcilerequestcollectionissue15.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth15](../models/syncreconcilerequesthealth15.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle15](../models/syncreconcilerequestlifecycle15.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |