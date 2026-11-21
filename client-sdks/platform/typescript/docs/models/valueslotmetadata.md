# ValueSlotMetadata

## Example Usage

```typescript
import { ValueSlotMetadata } from "@alienplatform/platform-api/models";

let value: ValueSlotMetadata = {
  slot: "<value>",
  kind: "secret",
  digest: "<value>",
  updatedAt: new Date("2025-08-19T11:15:20.099Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `slot`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Slot name: env:NAME for an environment variable, input:ID for a stack input.                  |
| `kind`                                                                                        | [models.EnvironmentVariableType](../models/environmentvariabletype.md)                        | :heavy_check_mark:                                                                            | Variable type (plain or secret)                                                               |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | Keyed digest returned by the manager when it stored the value.                                |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `targetResources`                                                                             | *string*[]                                                                                    | :heavy_minus_sign:                                                                            | Environment variables only: resources that get the variable (null = all).                     |