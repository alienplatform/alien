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
        mode: "autoscale",
        min: 509604,
        max: 214763,
      },
      recommended: {
        mode: "autoscale",
        min: 526675,
        max: 32921,
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
    },
  ],
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `pools`                            | [models.Pool](../models/pool.md)[] | :heavy_check_mark:                 | N/A                                |