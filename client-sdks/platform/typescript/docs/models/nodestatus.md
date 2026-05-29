# NodeStatus

## Example Usage

```typescript
import { NodeStatus } from "@alienplatform/platform-api/models";

let value: NodeStatus = {
  allocatable: {},
  capacity: {},
  labels: {
    "key": "<value>",
    "key1": "<value>",
  },
  name: "<value>",
  ready: true,
  roles: [],
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `allocatable`                                                    | [models.Allocatable](../models/allocatable.md)                   | :heavy_check_mark:                                               | N/A                                                              |
| `capacity`                                                       | [models.Capacity](../models/capacity.md)                         | :heavy_check_mark:                                               | N/A                                                              |
| `conditions`                                                     | [models.NodeStatusCondition](../models/nodestatuscondition.md)[] | :heavy_minus_sign:                                               | N/A                                                              |
| `containerRuntimeVersion`                                        | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `kubeletVersion`                                                 | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `labels`                                                         | Record<string, *string*>                                         | :heavy_check_mark:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `ready`                                                          | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `roles`                                                          | *string*[]                                                       | :heavy_check_mark:                                               | N/A                                                              |
| `uid`                                                            | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `usage`                                                          | *models.Usage*                                                   | :heavy_minus_sign:                                               | N/A                                                              |