# DeploymentMachine

## Example Usage

```typescript
import { DeploymentMachine } from "@alienplatform/platform-api/models";

let value: DeploymentMachine = {
  clusterResourceId: "<id>",
  machineId: "<id>",
  status: "<value>",
  capacityGroup: "<value>",
  zone: "<value>",
  cpu: {
    allocated: 2496.23,
    systemReserve: 4468.01,
    total: 3207.25,
  },
  memory: {
    allocated: 1238.58,
    systemReserve: 2770.56,
    total: 9405.72,
  },
  drainBlockers: [
    {
      reason: "<value>",
    },
  ],
  drainForce: true,
  lastHeartbeat: "<value>",
  localOverrides: [
    {
      baseAssignmentHash: "<value>",
      lifecycle: "<value>",
      replicaId: "<id>",
      workloadName: "<value>",
    },
  ],
  replicaCount: 396946,
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `clusterResourceId`                                                                  | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `machineId`                                                                          | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `status`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `capacityGroup`                                                                      | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `zone`                                                                               | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `cpu`                                                                                | [models.DeploymentMachineCpu](../models/deploymentmachinecpu.md)                     | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `memory`                                                                             | [models.DeploymentMachineMemory](../models/deploymentmachinememory.md)               | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `storage`                                                                            | [models.Storage](../models/storage.md)                                               | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `drainBlockers`                                                                      | [models.DeploymentMachineDrainBlocker](../models/deploymentmachinedrainblocker.md)[] | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `drainDeadlineAt`                                                                    | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `drainForce`                                                                         | *boolean*                                                                            | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `drainRequestedAt`                                                                   | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `drainedAt`                                                                          | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `publicIp`                                                                           | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `overlayIp`                                                                          | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `lastHeartbeat`                                                                      | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `horizondVersion`                                                                    | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `networkHealth`                                                                      | [models.NetworkHealth](../models/networkhealth.md)                                   | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `wireguardMesh`                                                                      | [models.WireguardMesh](../models/wireguardmesh.md)                                   | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `wireguardMeshObservedAt`                                                            | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `localOverrides`                                                                     | [models.LocalOverride](../models/localoverride.md)[]                                 | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `localOverridesObservedAt`                                                           | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `replicaCount`                                                                       | *number*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |