# DrainProgress4

## Example Usage

```typescript
import { DrainProgress4 } from "@alienplatform/platform-api/models";

let value: DrainProgress4 = {
  force: false,
  machineId: "<id>",
  replicaCount: 947901,
  stalled: true,
  status: "terminating",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `blockers`                                                       | [models.Blocker4](../models/blocker4.md)[]                       | :heavy_minus_sign:                                               | N/A                                                              |
| `drainDeadlineAt`                                                | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `drainRequestedAt`                                               | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `drainedAt`                                                      | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `force`                                                          | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `machineId`                                                      | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `replicaCount`                                                   | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `stalled`                                                        | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `status`                                                         | [models.DrainProgressStatus4](../models/drainprogressstatus4.md) | :heavy_check_mark:                                               | N/A                                                              |