# Capacity

## Example Usage

```typescript
import { Capacity } from "@alienplatform/platform-api/models/operations";

let value: Capacity = {
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
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `cpu`                                                                                          | [operations.GetDeploymentClusterCpu](../../models/operations/getdeploymentclustercpu.md)       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `memory`                                                                                       | [operations.GetDeploymentClusterMemory](../../models/operations/getdeploymentclustermemory.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `ephemeralStorage`                                                                             | [operations.EphemeralStorage](../../models/operations/ephemeralstorage.md)                     | :heavy_check_mark:                                                                             | N/A                                                                                            |