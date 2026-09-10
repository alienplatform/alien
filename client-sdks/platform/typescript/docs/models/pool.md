# Pool

## Example Usage

```typescript
import { Pool } from "@alienplatform/platform-api/models";

let value: Pool = {
  poolId: "<id>",
  workloads: [
    "<value 1>",
    "<value 2>",
  ],
  requirements: {
    cpu: "<value>",
    memoryBytes: 523395,
    ephemeralStorageBytes: 206903,
  },
  scale: {
    type: "autoscale",
    min: {
      min: 243735,
      max: 780745,
      default: 799327,
    },
    max: {
      min: 504244,
      max: 889408,
      default: 145999,
    },
  },
  selected: {
    max: 231434,
    min: 983977,
    mode: "autoscale",
  },
  recommended: {
    machines: 719795,
    mode: "fixed",
  },
  machines: [
    {
      machine: "<value>",
      profile: {
        cpu: "<value>",
        memoryBytes: 718877,
        ephemeralStorageBytes: 953830,
      },
      recommended: true,
    },
  ],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `poolId`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `workloads`                                                                        | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `requirements`                                                                     | [models.Requirements](../models/requirements.md)                                   | :heavy_check_mark:                                                                 | N/A                                                                                |
| `scale`                                                                            | *models.Scale*                                                                     | :heavy_check_mark:                                                                 | N/A                                                                                |
| `selected`                                                                         | *models.Selected*                                                                  | :heavy_check_mark:                                                                 | User-selected deployment settings for one compute pool.                            |
| `recommended`                                                                      | *models.Recommended*                                                               | :heavy_check_mark:                                                                 | User-selected deployment settings for one compute pool.                            |
| `machines`                                                                         | [models.DeploymentComputePlanMachine](../models/deploymentcomputeplanmachine.md)[] | :heavy_check_mark:                                                                 | N/A                                                                                |
| `errors`                                                                           | *string*[]                                                                         | :heavy_minus_sign:                                                                 | N/A                                                                                |