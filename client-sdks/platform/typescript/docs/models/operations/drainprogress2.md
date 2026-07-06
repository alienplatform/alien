# DrainProgress2

## Example Usage

```typescript
import { DrainProgress2 } from "@alienplatform/platform-api/models/operations";

let value: DrainProgress2 = {
  force: true,
  machineId: "<id>",
  replicaCount: 966205,
  stalled: false,
  status: "draining",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `blockers`                                                                         | [operations.Blocker2](../../models/operations/blocker2.md)[]                       | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainDeadlineAt`                                                                  | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainRequestedAt`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainedAt`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `force`                                                                            | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `machineId`                                                                        | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `replicaCount`                                                                     | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `stalled`                                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [operations.DrainProgressStatus2](../../models/operations/drainprogressstatus2.md) | :heavy_check_mark:                                                                 | N/A                                                                                |