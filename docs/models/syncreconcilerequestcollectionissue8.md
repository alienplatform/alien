# SyncReconcileRequestCollectionIssue8

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue8 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue8 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `message`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `reason`                                                                       | [models.SyncReconcileRequestReason8](../models/syncreconcilerequestreason8.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `severity`                                                                     | [models.StatusSeverity8](../models/statusseverity8.md)                         | :heavy_check_mark:                                                             | N/A                                                                            |
| `source`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |