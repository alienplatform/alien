# GetContainerOverviewResponse

Container overview across all deployments.

## Example Usage

```typescript
import { GetContainerOverviewResponse } from "@alienplatform/platform-api/models/operations";

let value: GetContainerOverviewResponse = {
  containerDefinitions: [],
  totals: {
    deployments: 653338,
    containerInstances: 295521,
    machines: 424735,
    machinesByStatus: {
      running: 439065,
      unhealthy: 394151,
      initializing: 457741,
      draining: 712653,
    },
    reschedulingFrozenCount: 711049,
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `containerDefinitions`                                                                         | [operations.ContainerDefinition](../../models/operations/containerdefinition.md)[]             | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `totals`                                                                                       | [operations.GetContainerOverviewTotals](../../models/operations/getcontaineroverviewtotals.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |