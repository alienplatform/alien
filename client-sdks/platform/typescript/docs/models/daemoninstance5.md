# DaemonInstance5

## Example Usage

```typescript
import { DaemonInstance5 } from "@alienplatform/platform-api/models";

let value: DaemonInstance5 = {
  kind: "daemon",
  name: "<value>",
  ready: true,
  unitId: "<id>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `cpu`                                                        | *models.DaemonInstanceCpuUnion5*                             | :heavy_minus_sign:                                           | N/A                                                          |
| `kind`                                                       | [models.DaemonInstanceKind](../models/daemoninstancekind.md) | :heavy_check_mark:                                           | N/A                                                          |
| `memory`                                                     | *models.DaemonInstanceMemoryUnion5*                          | :heavy_minus_sign:                                           | N/A                                                          |
| `name`                                                       | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `phase`                                                      | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `pid`                                                        | *number*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `ready`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `restartCount`                                               | *number*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `unitId`                                                     | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |