# SyncReconcileRequestEvent8

## Example Usage

```typescript
import { SyncReconcileRequestEvent8 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestEvent8 = {
  kind: "<value>",
  message: "<value>",
  observedAt: new Date("2024-04-11T18:17:44.358Z"),
  severity: "warning",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [models.EventSeverity8](../models/eventseverity8.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |