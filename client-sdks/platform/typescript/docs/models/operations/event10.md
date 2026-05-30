# Event10

## Example Usage

```typescript
import { Event10 } from "@alienplatform/platform-api/models/operations";

let value: Event10 = {
  kind: "<value>",
  message: "<value>",
  severity: "info",
  timestamp: new Date("2026-03-15T21:25:32.845Z"),
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