# DrainProgress1

## Example Usage

```typescript
import { DrainProgress1 } from "@alienplatform/platform-api/models/operations";

let value: DrainProgress1 = {
  force: true,
  machineId: "<id>",
  replicaCount: 964236,
  stalled: false,
  status: "draining",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `blockers`                                                                         | [operations.Blocker1](../../models/operations/blocker1.md)[]                       | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainDeadlineAt`                                                                  | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainRequestedAt`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainedAt`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `force`                                                                            | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `machineId`                                                                        | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `replicaCount`                                                                     | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `stalled`                                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [operations.DrainProgressStatus1](../../models/operations/drainprogressstatus1.md) | :heavy_check_mark:                                                                 | N/A                                                                                |