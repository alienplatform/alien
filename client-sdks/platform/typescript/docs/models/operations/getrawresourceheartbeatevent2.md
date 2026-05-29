# GetRawResourceHeartbeatEvent2

## Example Usage

```typescript
import { GetRawResourceHeartbeatEvent2 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatEvent2 = {
  kind: "<value>",
  message: "<value>",
  severity: "error",
  timestamp: new Date("2026-03-08T00:47:06.074Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [operations.EventSeverity1](../../models/operations/eventseverity1.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | *operations.SubjectUnion1*                                                                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |