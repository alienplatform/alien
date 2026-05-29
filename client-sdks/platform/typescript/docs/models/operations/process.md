# Process

## Example Usage

```typescript
import { Process } from "@alienplatform/platform-api/models/operations";

let value: Process = {
  kind: "process",
  name: "<value>",
  ready: false,
  unitId: "<id>",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `cpu`                                                            | *operations.ProcessCpuUnion*                                     | :heavy_minus_sign:                                               | N/A                                                              |
| `kind`                                                           | [operations.ProcessKind](../../models/operations/processkind.md) | :heavy_check_mark:                                               | N/A                                                              |
| `memory`                                                         | *operations.ProcessMemoryUnion*                                  | :heavy_minus_sign:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `phase`                                                          | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `pid`                                                            | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `ready`                                                          | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `restartCount`                                                   | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `unitId`                                                         | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |