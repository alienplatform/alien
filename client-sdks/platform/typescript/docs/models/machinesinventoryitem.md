# MachinesInventoryItem

## Example Usage

```typescript
import { MachinesInventoryItem } from "@alienplatform/platform-api/models";

let value: MachinesInventoryItem = {
  machineId: "<id>",
  status: "<value>",
  capacityGroup: "<value>",
  zone: "<value>",
  cpu: {
    allocated: 9516.82,
    systemReserve: 7650.68,
    total: 9994.17,
  },
  memory: {
    allocated: 4854.69,
    systemReserve: 2953.4,
    total: 9773.24,
  },
  drainBlockers: [],
  drainForce: true,
  lastHeartbeat: "<value>",
  localOverrides: [],
  replicaCount: 361878,
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `machineId`                                                                                | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `status`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `capacityGroup`                                                                            | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `zone`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `cpu`                                                                                      | [models.MachinesCapacityMetric](../models/machinescapacitymetric.md)                       | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `memory`                                                                                   | [models.MachinesCapacityMetric](../models/machinescapacitymetric.md)                       | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `storage`                                                                                  | [models.Storage](../models/storage.md)                                                     | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `drainBlockers`                                                                            | [models.MachinesDrainBlocker](../models/machinesdrainblocker.md)[]                         | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `drainDeadlineAt`                                                                          | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `drainForce`                                                                               | *boolean*                                                                                  | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `drainRequestedAt`                                                                         | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `drainedAt`                                                                                | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `publicIp`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `overlayIp`                                                                                | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `lastHeartbeat`                                                                            | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `horizondVersion`                                                                          | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `localOverrides`                                                                           | [models.MachinesLocalOverrideObservation](../models/machineslocaloverrideobservation.md)[] | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `localOverridesObservedAt`                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `replicaCount`                                                                             | *number*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |