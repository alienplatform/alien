# PutValue

## Example Usage

```typescript
import { PutValue } from "@alienplatform/manager-api/models";

let value: PutValue = {
  kind: "plain",
  slot: "<value>",
  value: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `kind`                                                       | [models.ValueKind](../models/valuekind.md)                   | :heavy_check_mark:                                           | Whether a value is shown back to people. Secrets never are.  |
| `slot`                                                       | *string*                                                     | :heavy_check_mark:                                           | `env:<NAME>`, `input:<id>` or `cenv.<containerName>.<NAME>`. |
| `value`                                                      | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
