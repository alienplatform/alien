# SyncReconcileRequestCollectionIssue1

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue1 = {
  message: "<value>",
  reason: "timed-out",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `message`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `reason`                                                                       | [models.SyncReconcileRequestReason1](../models/syncreconcilerequestreason1.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `severity`                                                                     | [models.StatusSeverity1](../models/statusseverity1.md)                         | :heavy_check_mark:                                                             | N/A                                                                            |
| `source`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |