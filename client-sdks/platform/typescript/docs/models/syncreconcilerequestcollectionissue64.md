# SyncReconcileRequestCollectionIssue64

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue64 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue64 = {
  message: "<value>",
  reason: "not-installed",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `message`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `reason`                                                                         | [models.SyncReconcileRequestReason64](../models/syncreconcilerequestreason64.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `severity`                                                                       | [models.StatusSeverity64](../models/statusseverity64.md)                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |