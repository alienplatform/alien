# Pod1

## Example Usage

```typescript
import { Pod1 } from "@alienplatform/platform-api/models";

let value: Pod1 = {
  name: "<value>",
  ownerReferences: [
    {
      controller: true,
      kind: "<value>",
      name: "<value>",
      uid: "<id>",
    },
  ],
  ready: false,
  restartCount: 238740,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `cpu`                                                    | *models.PodCpuUnion1*                                    | :heavy_minus_sign:                                       | N/A                                                      |
| `memory`                                                 | *models.PodMemoryUnion1*                                 | :heavy_minus_sign:                                       | N/A                                                      |
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