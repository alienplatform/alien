# SyncReconcileRequestMachine

## Example Usage

```typescript
import { SyncReconcileRequestMachine } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestMachine = {
  capacityGroup: "<value>",
  drainForce: true,
  lastHeartbeat: "<value>",
  machineId: "<id>",
  replicaCount: 34101,
  status: "<value>",
  zone: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `capacityGroup`                                    | *string*                                           | :heavy_check_mark:                                 | N/A                                                |
| `cpuCores`                                         | *number*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `drainBlockers`                                    | [models.DrainBlocker](../models/drainblocker.md)[] | :heavy_minus_sign:                                 | N/A                                                |
| `drainDeadlineAt`                                  | *string*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `drainForce`                                       | *boolean*                                          | :heavy_check_mark:                                 | N/A                                                |
| `drainRequestedAt`                                 | *string*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `drainedAt`                                        | *string*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `horizondVersion`                                  | *string*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `lastHeartbeat`                                    | *string*                                           | :heavy_check_mark:                                 | N/A                                                |
| `machineId`                                        | *string*                                           | :heavy_check_mark:                                 | N/A                                                |
| `memoryBytes`                                      | *number*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `overlayIp`                                        | *string*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `publicIp`                                         | *string*                                           | :heavy_minus_sign:                                 | N/A                                                |
| `replicaCount`                                     | *number*                                           | :heavy_check_mark:                                 | N/A                                                |
| `status`                                           | *string*                                           | :heavy_check_mark:                                 | N/A                                                |
| `zone`                                             | *string*                                           | :heavy_check_mark:                                 | N/A                                                |