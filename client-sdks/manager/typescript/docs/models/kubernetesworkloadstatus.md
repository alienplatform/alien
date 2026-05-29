# KubernetesWorkloadStatus

## Example Usage

```typescript
import { KubernetesWorkloadStatus } from "@alienplatform/manager-api/models";

let value: KubernetesWorkloadStatus = {
  conditions: [],
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `availableReplicas`                                                              | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `conditions`                                                                     | [models.KubernetesWorkloadCondition](../models/kubernetesworkloadcondition.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `desiredGeneration`                                                              | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `desiredReplicas`                                                                | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `observedGeneration`                                                             | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `readyReplicas`                                                                  | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `rolloutReason`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `updatedReplicas`                                                                | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |