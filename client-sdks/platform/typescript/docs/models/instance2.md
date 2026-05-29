# Instance2

## Example Usage

```typescript
import { Instance2 } from "@alienplatform/platform-api/models";

let value: Instance2 = {
  name: "<value>",
  ownerReferences: [
    {
      controller: true,
      kind: "<value>",
      name: "<value>",
      uid: "<id>",
    },
  ],
  ready: true,
  restartCount: 448038,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `cpu`                                                    | *models.InstanceCpuUnion2*                               | :heavy_minus_sign:                                       | N/A                                                      |
| `memory`                                                 | *models.InstanceMemoryUnion2*                            | :heavy_minus_sign:                                       | N/A                                                      |
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