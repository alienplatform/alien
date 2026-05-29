# GetRawResourceHeartbeatEvent5

## Example Usage

```typescript
import { GetRawResourceHeartbeatEvent5 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatEvent5 = {
  kind: "<value>",
  message: "<value>",
  severity: "warning",
  timestamp: new Date("2025-05-13T04:25:05.510Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [operations.EventSeverity2](../../models/operations/eventseverity2.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | *operations.SubjectUnion2*                                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |