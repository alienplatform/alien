# SyncReconcileRequestCollectionIssue21

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue21 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue21 = {
  message: "<value>",
  reason: "api-unavailable",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `message`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `reason`                                                                         | [models.SyncReconcileRequestReason21](../models/syncreconcilerequestreason21.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `severity`                                                                       | [models.StatusSeverity21](../models/statusseverity21.md)                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |