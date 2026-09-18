# Pod3

## Example Usage

```typescript
import { Pod3 } from "@alienplatform/platform-api/models/operations";

let value: Pod3 = {
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
  restartCount: 610035,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cpu`                                                                      | *operations.PodCpuUnion3*                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `memory`                                                                   | *operations.PodMemoryUnion3*                                               | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `nodeName`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `ownerReferences`                                                          | [operations.OwnerReference3](../../models/operations/ownerreference3.md)[] | :heavy_check_mark:                                                         | N/A                                                                        |
| `phase`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `podIp`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `ready`                                                                    | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `restartCount`                                                             | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `terminatedReason`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `uid`                                                                      | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `waitingReason`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |