# SyncReconcileRequestCollectionIssue2

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue2 = {
  message: "<value>",
  reason: "forbidden",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `message`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `reason`                                                                       | [models.SyncReconcileRequestReason2](../models/syncreconcilerequestreason2.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `severity`                                                                     | [models.StatusSeverity2](../models/statusseverity2.md)                         | :heavy_check_mark:                                                             | N/A                                                                            |
| `source`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |