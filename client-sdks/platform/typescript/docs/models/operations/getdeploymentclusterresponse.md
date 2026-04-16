# GetDeploymentClusterResponse

Cluster overview.

## Example Usage

```typescript
import { GetDeploymentClusterResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentClusterResponse = {
  machineCount: 931058,
  containerCount: 479558,
  capacity: {
    cpu: {
      total: 3331.22,
      available: 5699.93,
    },
    memory: {
      total: 665726,
      available: 412412,
    },
    ephemeralStorage: {
      total: 614890,
      available: 231356,
    },
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `machineCount`                                             | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `containerCount`                                           | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `capacity`                                                 | [operations.Capacity](../../models/operations/capacity.md) | :heavy_check_mark:                                         | N/A                                                        |