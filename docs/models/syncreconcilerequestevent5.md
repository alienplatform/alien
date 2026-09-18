# SyncReconcileRequestEvent5

## Example Usage

```typescript
import { SyncReconcileRequestEvent5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestEvent5 = {
  kind: "<value>",
  message: "<value>",
  severity: "warning",
  timestamp: new Date("2024-01-30T15:46:55.202Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [models.EventSeverity2](../models/eventseverity2.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | *models.SyncReconcileRequestSubjectUnion2*                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |