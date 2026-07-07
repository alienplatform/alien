# DrainProgress3

## Example Usage

```typescript
import { DrainProgress3 } from "@alienplatform/platform-api/models/operations";

let value: DrainProgress3 = {
  force: false,
  machineId: "<id>",
  replicaCount: 940240,
  stalled: false,
  status: "draining",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `blockers`                                                                         | [operations.Blocker3](../../models/operations/blocker3.md)[]                       | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainDeadlineAt`                                                                  | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainRequestedAt`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `drainedAt`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `force`                                                                            | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `machineId`                                                                        | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `replicaCount`                                                                     | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `stalled`                                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [operations.DrainProgressStatus3](../../models/operations/drainprogressstatus3.md) | :heavy_check_mark:                                                                 | N/A                                                                                |