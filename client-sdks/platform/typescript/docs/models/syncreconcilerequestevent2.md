# SyncReconcileRequestEvent2

## Example Usage

```typescript
import { SyncReconcileRequestEvent2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestEvent2 = {
  kind: "<value>",
  message: "<value>",
  severity: "info",
  timestamp: new Date("2025-12-01T00:17:44.411Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [models.EventSeverity1](../models/eventseverity1.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | *models.SyncReconcileRequestSubjectUnion1*                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |