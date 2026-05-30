# Event5

## Example Usage

```typescript
import { Event5 } from "@alienplatform/platform-api/models/operations";

let value: Event5 = {
  kind: "<value>",
  message: "<value>",
  severity: "info",
  timestamp: new Date("2025-06-12T02:16:49.007Z"),
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