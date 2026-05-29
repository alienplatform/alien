# GetRawResourceHeartbeatEvent13

## Example Usage

```typescript
import { GetRawResourceHeartbeatEvent13 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatEvent13 = {
  kind: "<value>",
  message: "<value>",
  observedAt: new Date("2025-02-24T14:02:42.621Z"),
  severity: "warning",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [operations.EventSeverity13](../../models/operations/eventseverity13.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |