# SyncReconcileRequestCollectionIssue31

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue31 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue31 = {
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
| `reason`                                                                         | [models.SyncReconcileRequestReason31](../models/syncreconcilerequestreason31.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `severity`                                                                       | [models.StatusSeverity31](../models/statusseverity31.md)                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |