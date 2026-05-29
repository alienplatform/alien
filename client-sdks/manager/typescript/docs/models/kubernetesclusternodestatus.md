# KubernetesClusterNodeStatus

## Example Usage

```typescript
import { KubernetesClusterNodeStatus } from "@alienplatform/manager-api/models";

let value: KubernetesClusterNodeStatus = {
  allocatable: {},
  capacity: {},
  labels: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  name: "<value>",
  ready: false,
  roles: [],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `allocatable`                                                                        | [models.KubernetesNodeResources](../models/kubernetesnoderesources.md)               | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `capacity`                                                                           | [models.KubernetesNodeResources](../models/kubernetesnoderesources.md)               | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `conditions`                                                                         | [models.KubernetesNodeConditionStatus](../models/kubernetesnodeconditionstatus.md)[] | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `containerRuntimeVersion`                                                            | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `kubeletVersion`                                                                     | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `labels`                                                                             | Record<string, *string*>                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `name`                                                                               | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `ready`                                                                              | *boolean*                                                                            | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `roles`                                                                              | *string*[]                                                                           | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `uid`                                                                                | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `usage`                                                                              | [models.KubernetesNodeUsage](../models/kubernetesnodeusage.md)                       | :heavy_minus_sign:                                                                   | N/A                                                                                  |