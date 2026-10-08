# Workload1

## Example Usage

```typescript
import { Workload1 } from "@alienplatform/platform-api/models";

let value: Workload1 = {
  conditions: [
    {
      status: "<value>",
      type: "<value>",
    },
  ],
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `availableReplicas`                                            | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `conditions`                                                   | [models.WorkloadCondition1](../models/workloadcondition1.md)[] | :heavy_check_mark:                                             | N/A                                                            |
| `desiredGeneration`                                            | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `desiredReplicas`                                              | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `observedGeneration`                                           | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `readyReplicas`                                                | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `rolloutReason`                                                | *string*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `updatedReplicas`                                              | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |