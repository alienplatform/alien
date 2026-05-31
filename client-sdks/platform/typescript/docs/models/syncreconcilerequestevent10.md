# SyncReconcileRequestEvent10

## Example Usage

```typescript
import { SyncReconcileRequestEvent10 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestEvent10 = {
  kind: "<value>",
  message: "<value>",
  severity: "error",
  timestamp: new Date("2025-09-17T11:35:20.279Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [models.EventSeverity3](../models/eventseverity3.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | *models.SyncReconcileRequestSubjectUnion3*                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |