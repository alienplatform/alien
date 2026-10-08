# DaemonInstance5

## Example Usage

```typescript
import { DaemonInstance5 } from "@alienplatform/platform-api/models/operations";

let value: DaemonInstance5 = {
  kind: "daemon",
  name: "<value>",
  ready: true,
  unitId: "<id>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `cpu`                                                                          | *operations.DaemonInstanceCpuUnion5*                                           | :heavy_minus_sign:                                                             | N/A                                                                            |
| `kind`                                                                         | [operations.DaemonInstanceKind](../../models/operations/daemoninstancekind.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *operations.DaemonInstanceMemoryUnion5*                                        | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `phase`                                                                        | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `pid`                                                                          | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `ready`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `restartCount`                                                                 | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `unitId`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |