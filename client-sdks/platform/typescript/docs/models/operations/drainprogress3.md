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

| Field              | Type                                                                               | Required           | Description |
| ------------------ | ---------------------------------------------------------------------------------- | ------------------ | ----------- |
| `blockers`         | [operations.Blocker3](../../models/operations/blocker3.md)[]                       | :heavy_minus_sign: | N/A         |
| `drainDeadlineAt`  | _string_                                                                           | :heavy_minus_sign: | N/A         |
| `drainRequestedAt` | _string_                                                                           | :heavy_minus_sign: | N/A         |
| `drainedAt`        | _string_                                                                           | :heavy_minus_sign: | N/A         |
| `force`            | _boolean_                                                                          | :heavy_check_mark: | N/A         |
| `machineId`        | _string_                                                                           | :heavy_check_mark: | N/A         |
| `replicaCount`     | _number_                                                                           | :heavy_check_mark: | N/A         |
| `stalled`          | _boolean_                                                                          | :heavy_check_mark: | N/A         |
| `status`           | [operations.DrainProgressStatus3](../../models/operations/drainprogressstatus3.md) | :heavy_check_mark: | N/A         |
