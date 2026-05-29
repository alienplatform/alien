# Instance1

## Example Usage

```typescript
import { Instance1 } from "@alienplatform/platform-api/models";

let value: Instance1 = {
  name: "<value>",
  ownerReferences: [],
  ready: false,
  restartCount: 887147,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `cpu`                                                    | *models.InstanceCpuUnion1*                               | :heavy_minus_sign:                                       | N/A                                                      |
| `memory`                                                 | *models.InstanceMemoryUnion1*                            | :heavy_minus_sign:                                       | N/A                                                      |
| `name`                                                   | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `nodeName`                                               | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `ownerReferences`                                        | [models.OwnerReference1](../models/ownerreference1.md)[] | :heavy_check_mark:                                       | N/A                                                      |
| `phase`                                                  | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `podIp`                                                  | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `ready`                                                  | *boolean*                                                | :heavy_check_mark:                                       | N/A                                                      |
| `restartCount`                                           | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `terminatedReason`                                       | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `uid`                                                    | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `waitingReason`                                          | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |