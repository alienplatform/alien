# DrainProgress2

## Example Usage

```typescript
import { DrainProgress2 } from "@alienplatform/platform-api/models";

let value: DrainProgress2 = {
  force: true,
  machineId: "<id>",
  replicaCount: 966205,
  stalled: false,
  status: "draining",
};
```

## Fields

| Field              | Type                                                             | Required           | Description |
| ------------------ | ---------------------------------------------------------------- | ------------------ | ----------- |
| `blockers`         | [models.Blocker2](../models/blocker2.md)[]                       | :heavy_minus_sign: | N/A         |
| `drainDeadlineAt`  | _string_                                                         | :heavy_minus_sign: | N/A         |
| `drainRequestedAt` | _string_                                                         | :heavy_minus_sign: | N/A         |
| `drainedAt`        | _string_                                                         | :heavy_minus_sign: | N/A         |
| `force`            | _boolean_                                                        | :heavy_check_mark: | N/A         |
| `machineId`        | _string_                                                         | :heavy_check_mark: | N/A         |
| `replicaCount`     | _number_                                                         | :heavy_check_mark: | N/A         |
| `stalled`          | _boolean_                                                        | :heavy_check_mark: | N/A         |
| `status`           | [models.DrainProgressStatus2](../models/drainprogressstatus2.md) | :heavy_check_mark: | N/A         |
