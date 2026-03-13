# GetContainerMachinesResponse

Machine health across all deployments.

## Example Usage

```typescript
import { GetContainerMachinesResponse } from "@aliendotdev/platform-api/models/operations";

let value: GetContainerMachinesResponse = {
  deployments: [],
  totals: {
    machines: 779619,
    machinesByStatus: {
      running: 941694,
      unhealthy: 424809,
      initializing: 323712,
      draining: 542030,
    },
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `deployments`                                                                                            | [operations.GetContainerMachinesDeployment](../../models/operations/getcontainermachinesdeployment.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `totals`                                                                                                 | [operations.GetContainerMachinesTotals](../../models/operations/getcontainermachinestotals.md)           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |