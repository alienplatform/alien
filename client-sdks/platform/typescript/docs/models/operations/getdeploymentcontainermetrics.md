# GetDeploymentContainerMetrics

## Example Usage

```typescript
import { GetDeploymentContainerMetrics } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentContainerMetrics = {
  status: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `cpuUsage`                                                                                    | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `cpuUsagePercent`                                                                             | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `memoryUsedBytes`                                                                             | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `memoryUsagePercent`                                                                          | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `healthy`                                                                                     | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `lastUpdated`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `http`                                                                                        | [operations.Http](../../models/operations/http.md)                                            | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `additionalProperties`                                                                        | Record<string, *any*>                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |