# SyncReconcileRequestCollectionIssue42

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue42 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue42 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `message`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `reason`                                                                         | [models.SyncReconcileRequestReason42](../models/syncreconcilerequestreason42.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `severity`                                                                       | [models.StatusSeverity42](../models/statusseverity42.md)                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |