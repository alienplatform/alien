# GetRawResourceHeartbeatInstance6

## Example Usage

```typescript
import { GetRawResourceHeartbeatInstance6 } from "@alienplatform/platform-api/models/operations";

let value: GetRawResourceHeartbeatInstance6 = {
  name: "<value>",
  ownerReferences: [
    {
      controller: false,
      kind: "<value>",
      name: "<value>",
      uid: "<id>",
    },
  ],
  ready: false,
  restartCount: 106101,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cpu`                                                                      | *operations.InstanceCpuUnion6*                                             | :heavy_minus_sign:                                                         | N/A                                                                        |
| `memory`                                                                   | *operations.InstanceMemoryUnion6*                                          | :heavy_minus_sign:                                                         | N/A                                                                        |
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