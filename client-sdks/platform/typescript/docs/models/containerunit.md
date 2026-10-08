# ContainerUnit

## Example Usage

```typescript
import { ContainerUnit } from "@alienplatform/platform-api/models";

let value: ContainerUnit = {
  kind: "process",
  name: "<value>",
  ready: true,
  unitId: "<id>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `cpu`                                                      | *models.ContainerUnitCpuUnion*                             | :heavy_minus_sign:                                         | N/A                                                        |
| `kind`                                                     | [models.ContainerUnitKind](../models/containerunitkind.md) | :heavy_check_mark:                                         | N/A                                                        |
| `memory`                                                   | *models.ContainerUnitMemoryUnion*                          | :heavy_minus_sign:                                         | N/A                                                        |
| `name`                                                     | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `phase`                                                    | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `pid`                                                      | *number*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `ready`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `restartCount`                                             | *number*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `unitId`                                                   | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |