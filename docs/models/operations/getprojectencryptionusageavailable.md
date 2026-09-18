# GetProjectEncryptionUsageAvailable

## Example Usage

```typescript
import { GetProjectEncryptionUsageAvailable } from "@alienplatform/platform-api/models/operations";

let value: GetProjectEncryptionUsageAvailable = {
  status: "available",
  range: "24h",
  totals: {
    requests: 658481,
    successfulRequests: 171123,
    errorRequests: 236198,
    averageLatencyMs: 1844.76,
    p95LatencyMs: 8822.27,
  },
  timeSeries: [
    {
      requests: 342099,
      successfulRequests: 364793,
      errorRequests: 447048,
      averageLatencyMs: null,
      p95LatencyMs: 3635.63,
      bucket: new Date("2026-04-03T03:09:28.224Z"),
    },
  ],
  operations: [
    {
      requests: 130308,
      successfulRequests: 642619,
      errorRequests: 734494,
      averageLatencyMs: 8308,
      p95LatencyMs: 1903.33,
      operation: "decrypt",
    },
  ],
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `status`                                                                                                               | *"available"*                                                                                                          | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `range`                                                                                                                | [operations.GetProjectEncryptionUsageRangeResponse](../../models/operations/getprojectencryptionusagerangeresponse.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `totals`                                                                                                               | [operations.GetProjectEncryptionUsageTotals](../../models/operations/getprojectencryptionusagetotals.md)               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `timeSeries`                                                                                                           | [operations.GetProjectEncryptionUsageTimeSery](../../models/operations/getprojectencryptionusagetimesery.md)[]         | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `operations`                                                                                                           | [operations.Operation](../../models/operations/operation.md)[]                                                         | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |