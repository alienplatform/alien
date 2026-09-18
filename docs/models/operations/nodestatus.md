# NodeStatus

## Example Usage

```typescript
import { NodeStatus } from "@alienplatform/platform-api/models/operations";

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

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `allocatable`                                                                      | [operations.Allocatable](../../models/operations/allocatable.md)                   | :heavy_check_mark:                                                                 | N/A                                                                                |
| `capacity`                                                                         | [operations.Capacity](../../models/operations/capacity.md)                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `conditions`                                                                       | [operations.NodeStatusCondition](../../models/operations/nodestatuscondition.md)[] | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `containerRuntimeVersion`                                                          | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `kubeletVersion`                                                                   | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `labels`                                                                           | Record<string, *string*>                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `ready`                                                                            | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `roles`                                                                            | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `uid`                                                                              | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `usage`                                                                            | *operations.UsageUnion*                                                            | :heavy_minus_sign:                                                                 | N/A                                                                                |