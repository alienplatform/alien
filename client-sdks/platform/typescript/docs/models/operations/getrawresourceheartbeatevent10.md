# GetRawResourceHeartbeatEvent10

## Example Usage

```typescript
import { GetRawResourceHeartbeatEvent10 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatEvent10 = {
  kind: "<value>",
  message: "<value>",
  severity: "info",
  timestamp: new Date("2025-09-17T07:36:48.019Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [operations.EventSeverity3](../../models/operations/eventseverity3.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | *operations.SubjectUnion3*                                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |