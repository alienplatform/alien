# GetRawResourceHeartbeatEvent2

## Example Usage

```typescript
import { GetRawResourceHeartbeatEvent2 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatEvent2 = {
  kind: "<value>",
  message: "<value>",
  observedAt: new Date("2026-12-01T03:18:15.365Z"),
  severity: "error",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [operations.EventSeverity2](../../models/operations/eventseverity2.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |