# SyncReconcileRequestStatus22

## Example Usage

```typescript
import { SyncReconcileRequestStatus22 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus22 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "unknown",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue22](../models/syncreconcilerequestcollectionissue22.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth22](../models/syncreconcilerequesthealth22.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle22](../models/syncreconcilerequestlifecycle22.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |