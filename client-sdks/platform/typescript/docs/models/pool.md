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
    mode: "autoscale",
    min: 231434,
    max: 983977,
  },
  recommended: {
    mode: "fixed",
    machines: 719795,
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
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `poolId`                                         | *string*                                         | :heavy_check_mark:                               | N/A                                              |
| `workloads`                                      | *string*[]                                       | :heavy_check_mark:                               | N/A                                              |
| `requirements`                                   | [models.Requirements](../models/requirements.md) | :heavy_check_mark:                               | N/A                                              |
| `scale`                                          | *models.Scale*                                   | :heavy_check_mark:                               | N/A                                              |
| `selected`                                       | *models.Selected*                                | :heavy_check_mark:                               | N/A                                              |
| `recommended`                                    | *models.Recommended*                             | :heavy_check_mark:                               | N/A                                              |
| `machines`                                       | [models.Machine](../models/machine.md)[]         | :heavy_check_mark:                               | N/A                                              |
| `errors`                                         | *string*[]                                       | :heavy_minus_sign:                               | N/A                                              |