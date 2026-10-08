# SyncReconcileRequestCollectionIssue20

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue20 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue20 = {
  message: "<value>",
  reason: "api-unavailable",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `message`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `reason`                                                                         | [models.SyncReconcileRequestReason20](../models/syncreconcilerequestreason20.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `severity`                                                                       | [models.StatusSeverity20](../models/statusseverity20.md)                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |