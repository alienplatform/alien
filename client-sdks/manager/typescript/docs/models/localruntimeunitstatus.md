# LocalRuntimeUnitStatus

## Example Usage

```typescript
import { LocalRuntimeUnitStatus } from "@alienplatform/manager-api/models";

let value: LocalRuntimeUnitStatus = {
  kind: "container",
  name: "<value>",
  ready: true,
  unitId: "<id>",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `cpu`                                                            | [models.MetricSample](../models/metricsample.md)                 | :heavy_minus_sign:                                               | N/A                                                              |
| `kind`                                                           | [models.LocalRuntimeUnitKind](../models/localruntimeunitkind.md) | :heavy_check_mark:                                               | N/A                                                              |
| `memory`                                                         | [models.MetricSample](../models/metricsample.md)                 | :heavy_minus_sign:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `phase`                                                          | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `pid`                                                            | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `ready`                                                          | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `restartCount`                                                   | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `unitId`                                                         | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |