# GetDeploymentMetricsHistoryResponse

Metrics history

## Example Usage

```typescript
import { GetDeploymentMetricsHistoryResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentMetricsHistoryResponse = {
  requiresFilter: false,
  bucketSeconds: 7226.89,
  services: [
    "<value 1>",
    "<value 2>",
  ],
  replicas: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  series: [
    {
      resourceId: "<id>",
      replicaId: "<id>",
      points: [],
    },
  ],
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `requiresFilter`                                          | *boolean*                                                 | :heavy_check_mark:                                        | More than 64 replicas match. Select a service or replica. |
| `bucketSeconds`                                           | *number*                                                  | :heavy_check_mark:                                        | N/A                                                       |
| `services`                                                | *string*[]                                                | :heavy_check_mark:                                        | N/A                                                       |
| `replicas`                                                | *string*[]                                                | :heavy_check_mark:                                        | N/A                                                       |
| `series`                                                  | [operations.Series](../../models/operations/series.md)[]  | :heavy_check_mark:                                        | N/A                                                       |