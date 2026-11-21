# ValueRef

## Example Usage

```typescript
import { ValueRef } from "@alienplatform/platform-api/models";

let value: ValueRef = {
  slot: "<value>",
  kind: "secret",
  digest: "<value>",
  updatedAt: new Date("2024-11-30T01:32:01.958Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `slot`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Slot name: env:NAME for an environment variable, input:ID for a stack input.                  |
| `kind`                                                                                        | [models.EnvironmentVariableType](../models/environmentvariabletype.md)                        | :heavy_check_mark:                                                                            | Variable type (plain or secret)                                                               |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |