# SyncReconcileRequestCollectionIssue3

## Example Usage

```typescript
import { SyncReconcileRequestCollectionIssue3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCollectionIssue3 = {
  message: "<value>",
  reason: "not-installed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `message`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `reason`                                                                       | [models.SyncReconcileRequestReason3](../models/syncreconcilerequestreason3.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `severity`                                                                     | [models.StatusSeverity3](../models/statusseverity3.md)                         | :heavy_check_mark:                                                             | N/A                                                                            |
| `source`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |