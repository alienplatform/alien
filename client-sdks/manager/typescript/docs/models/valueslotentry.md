# ValueSlotEntry

## Example Usage

```typescript
import { ValueSlotEntry } from "@alienplatform/manager-api/models";

let value: ValueSlotEntry = {
  digest: "<value>",
  kind: "secret",
  slot: "<value>",
  updatedAt: "1735617005412",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `digest`                                                    | *string*                                                    | :heavy_check_mark:                                          | Keyed digest of the value, lowercase hex.                   |
| `kind`                                                      | [models.ValueKind](../models/valuekind.md)                  | :heavy_check_mark:                                          | Whether a value is shown back to people. Secrets never are. |
| `slot`                                                      | *string*                                                    | :heavy_check_mark:                                          | N/A                                                         |
| `updatedAt`                                                 | *string*                                                    | :heavy_check_mark:                                          | N/A                                                         |
| `value`                                                     | *string*                                                    | :heavy_minus_sign:                                          | The value, for plain slots only.                            |
