# GetContainerMachinesDeployment

## Example Usage

```typescript
import { GetContainerMachinesDeployment } from "@alienplatform/platform-api/models/operations";

let value: GetContainerMachinesDeployment = {
  clusterId: "<id>",
  deploymentId: "<id>",
  deploymentName: "<value>",
  totalMachines: 112500,
  machinesByStatus: {
    running: 227766,
    unhealthy: 781107,
    initializing: 721736,
    draining: 76692,
  },
  capacityGroups: [],
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `clusterId`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `deploymentId`                                                                                 | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `deploymentName`                                                                               | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `deploymentGroupId`                                                                            | *string*                                                                                       | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `deploymentGroupName`                                                                          | *string*                                                                                       | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `projectName`                                                                                  | *string*                                                                                       | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `totalMachines`                                                                                | *number*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `machinesByStatus`                                                                             | [operations.DeploymentMachinesByStatus](../../models/operations/deploymentmachinesbystatus.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `capacityGroups`                                                                               | [operations.CapacityGroup](../../models/operations/capacitygroup.md)[]                         | :heavy_check_mark:                                                                             | N/A                                                                                            |