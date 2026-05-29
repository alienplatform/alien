# KubernetesPodInstanceStatus

## Example Usage

```typescript
import { KubernetesPodInstanceStatus } from "@alienplatform/manager-api/models";

let value: KubernetesPodInstanceStatus = {
  name: "<value>",
  ownerReferences: [
    {
      controller: true,
      kind: "<value>",
      name: "<value>",
      uid: "<id>",
    },
  ],
  ready: false,
  restartCount: 422280,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cpu`                                                                      | [models.MetricSample](../models/metricsample.md)                           | :heavy_minus_sign:                                                         | N/A                                                                        |
| `memory`                                                                   | [models.MetricSample](../models/metricsample.md)                           | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `nodeName`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `ownerReferences`                                                          | [models.KubernetesOwnerReference](../models/kubernetesownerreference.md)[] | :heavy_check_mark:                                                         | N/A                                                                        |
| `phase`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `podIp`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `ready`                                                                    | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `restartCount`                                                             | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `terminatedReason`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `uid`                                                                      | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `waitingReason`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |