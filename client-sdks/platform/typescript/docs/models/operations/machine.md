# Machine

## Example Usage

```typescript
import { Machine } from "@alienplatform/platform-api/models/operations";

let value: Machine = {
  machineId: "<id>",
  capacityGroup: "<value>",
  zone: "<value>",
  status: "running",
  cpu: {
    total: 7946.62,
    systemReserve: 7976.32,
    allocated: 905.61,
  },
  memory: {
    total: 309543,
    systemReserve: 760949,
    allocated: 546260,
  },
  replicaCount: 875180,
  lastHeartbeat: "<value>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `machineId`                                                                                        | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `capacityGroup`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `zone`                                                                                             | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `status`                                                                                           | [operations.ListDeploymentMachinesStatus](../../models/operations/listdeploymentmachinesstatus.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `cpu`                                                                                              | [operations.ListDeploymentMachinesCpu](../../models/operations/listdeploymentmachinescpu.md)       | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `memory`                                                                                           | [operations.ListDeploymentMachinesMemory](../../models/operations/listdeploymentmachinesmemory.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `gpu`                                                                                              | [operations.ListDeploymentMachinesGpu](../../models/operations/listdeploymentmachinesgpu.md)       | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `replicaCount`                                                                                     | *number*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `lastHeartbeat`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |