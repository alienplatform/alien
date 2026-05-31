# Workload2

## Example Usage

```typescript
import { Workload2 } from "@alienplatform/platform-api/models";

let value: Workload2 = {
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
| `conditions`                                                   | [models.WorkloadCondition2](../models/workloadcondition2.md)[] | :heavy_check_mark:                                             | N/A                                                            |
| `desiredGeneration`                                            | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `desiredReplicas`                                              | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `observedGeneration`                                           | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `readyReplicas`                                                | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `rolloutReason`                                                | *string*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `updatedReplicas`                                              | *number*                                                       | :heavy_minus_sign:                                             | N/A                                                            |