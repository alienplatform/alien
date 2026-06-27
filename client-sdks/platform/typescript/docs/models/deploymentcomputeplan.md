# DeploymentComputePlan

## Example Usage

```typescript
import { DeploymentComputePlan } from "@alienplatform/platform-api/models";

let value: DeploymentComputePlan = {
  pools: [
    {
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
      selected: {
        mode: "autoscale",
        min: 591224,
        max: 509604,
      },
      recommended: {
        mode: "fixed",
        machines: 736018,
      },
      machines: [
        {
          machine: "<value>",
          profile: {
            cpu: "<value>",
            memoryBytes: 325948,
            ephemeralStorageBytes: 794662,
          },
          recommended: true,
        },
      ],
    },
  ],
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `pools`                            | [models.Pool](../models/pool.md)[] | :heavy_check_mark:                 | N/A                                |