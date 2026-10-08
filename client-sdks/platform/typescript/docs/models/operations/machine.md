# Machine

## Example Usage

```typescript
import { Machine } from "@alienplatform/platform-api/models/operations";

let value: Machine = {
  capacityGroup: "<value>",
  drainForce: true,
  lastHeartbeat: "<value>",
  machineId: "<id>",
  replicaCount: 794662,
  status: "<value>",
  zone: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `capacityGroup`                                                      | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `cpuCores`                                                           | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `drainBlockers`                                                      | [operations.DrainBlocker](../../models/operations/drainblocker.md)[] | :heavy_minus_sign:                                                   | N/A                                                                  |
| `drainDeadlineAt`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `drainForce`                                                         | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `drainRequestedAt`                                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `drainedAt`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `horizondVersion`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `lastHeartbeat`                                                      | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `machineId`                                                          | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `memoryBytes`                                                        | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `overlayIp`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `publicIp`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `replicaCount`                                                       | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `status`                                                             | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `zone`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |