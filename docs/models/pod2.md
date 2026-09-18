# Pod2

## Example Usage

```typescript
import { Pod2 } from "@alienplatform/platform-api/models";

let value: Pod2 = {
  name: "<value>",
  ownerReferences: [],
  ready: true,
  restartCount: 806073,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `cpu`                                                    | *models.PodCpuUnion2*                                    | :heavy_minus_sign:                                       | N/A                                                      |
| `memory`                                                 | *models.PodMemoryUnion2*                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `name`                                                   | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `nodeName`                                               | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `ownerReferences`                                        | [models.OwnerReference2](../models/ownerreference2.md)[] | :heavy_check_mark:                                       | N/A                                                      |
| `phase`                                                  | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `podIp`                                                  | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `ready`                                                  | *boolean*                                                | :heavy_check_mark:                                       | N/A                                                      |
| `restartCount`                                           | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `terminatedReason`                                       | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `uid`                                                    | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `waitingReason`                                          | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |