# SyncReconcileRequestCollectionIssue7

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue7 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue7 = {
  message: "<value>",
  reason: "api-unavailable",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `message`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `reason`                                                                       | [models.SyncReconcileRequestReason7](../models/syncreconcilerequestreason7.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `severity`                                                                     | [models.StatusSeverity7](../models/statusseverity7.md)                         | :heavy_check_mark:                                                             | N/A                                                                            |
| `source`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |