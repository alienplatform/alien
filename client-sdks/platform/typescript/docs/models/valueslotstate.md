# ValueSlotState

## Example Usage

```typescript
import { ValueSlotState } from "@alienplatform/platform-api/models";

let value: ValueSlotState = {
  slot: "<value>",
  kind: "secret",
  set: false,
  digest: "<value>",
  updatedAt: new Date("2026-07-14T04:32:48.382Z"),
  updatedBy: null,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `slot`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Slot name: env:NAME for an environment variable, input:ID for a stack input.                  |
| `kind`                                                                                        | [models.EnvironmentVariableType](../models/environmentvariabletype.md)                        | :heavy_check_mark:                                                                            | Variable type (plain or secret)                                                               |
| `set`                                                                                         | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | Null while the value is still stored on the platform (older path).                            |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedBy`                                                                                   | [models.ValueSlotStateUpdatedBy](../models/valueslotstateupdatedby.md)                        | :heavy_check_mark:                                                                            | Who stored the value, from the authenticated request.                                         |