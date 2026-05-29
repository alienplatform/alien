# GetRawResourceHeartbeatEvent17

## Example Usage

```typescript
import { GetRawResourceHeartbeatEvent17 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatEvent17 = {
  kind: "<value>",
  message: "<value>",
  observedAt: new Date("2026-08-05T00:26:43.844Z"),
  severity: "error",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [operations.EventSeverity17](../../models/operations/eventseverity17.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |