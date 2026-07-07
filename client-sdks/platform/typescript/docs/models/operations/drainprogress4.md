# DrainProgress4

## Example Usage

```typescript
import { DrainProgress4 } from "@alienplatform/platform-api/models/operations";

let value: DrainProgress4 = {
  force: false,
  machineId: "<id>",
  replicaCount: 947901,
  stalled: true,
  status: "terminating",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `blockers`                                                                         | [operations.Blocker4](../../models/operations/blocker4.md)[]                       | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainDeadlineAt`                                                                  | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainRequestedAt`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainedAt`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `force`                                                                            | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `machineId`                                                                        | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `replicaCount`                                                                     | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `stalled`                                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [operations.DrainProgressStatus4](../../models/operations/drainprogressstatus4.md) | :heavy_check_mark:                                                                 | N/A                                                                                |