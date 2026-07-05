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

| Field              | Type                                                                               | Required           | Description |
| ------------------ | ---------------------------------------------------------------------------------- | ------------------ | ----------- |
| `blockers`         | [operations.Blocker1](../../models/operations/blocker1.md)[]                       | :heavy_minus_sign: | N/A         |
| `drainDeadlineAt`  | _string_                                                                           | :heavy_minus_sign: | N/A         |
| `drainRequestedAt` | _string_                                                                           | :heavy_minus_sign: | N/A         |
| `drainedAt`        | _string_                                                                           | :heavy_minus_sign: | N/A         |
| `force`            | _boolean_                                                                          | :heavy_check_mark: | N/A         |
| `machineId`        | _string_                                                                           | :heavy_check_mark: | N/A         |
| `replicaCount`     | _number_                                                                           | :heavy_check_mark: | N/A         |
| `stalled`          | _boolean_                                                                          | :heavy_check_mark: | N/A         |
| `status`           | [operations.DrainProgressStatus1](../../models/operations/drainprogressstatus1.md) | :heavy_check_mark: | N/A         |
