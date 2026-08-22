# GetProjectAiUsageAvailable

## Example Usage

```typescript
import { GetProjectAiUsageAvailable } from "@alienplatform/platform-api/models/operations";

let value: GetProjectAiUsageAvailable = {
  status: "available",
  range: "24h",
  pricing: {
    label: "Estimated provider cost",
    coverage: 9612.62,
    revision: "<value>",
  },
  totals: {
    requests: 983848,
    successfulRequests: 829998,
    errorRequests: 221988,
    averageLatencyMs: null,
    p95LatencyMs: 5834.6,
    inputTokens: 119359,
    outputTokens: 938751,
    estimatedCostMicrousd: null,
    pricedRequests: 171593,
  },
  timeSeries: [],
  customers: [],
  models: [],
  customersTruncated: false,
  modelsTruncated: false,
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `status`                                                                                               | *"available"*                                                                                          | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `range`                                                                                                | [operations.GetProjectAiUsageRangeResponse](../../models/operations/getprojectaiusagerangeresponse.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `pricing`                                                                                              | [operations.Pricing](../../models/operations/pricing.md)                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `totals`                                                                                               | [operations.GetProjectAiUsageTotals](../../models/operations/getprojectaiusagetotals.md)               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `timeSeries`                                                                                           | [operations.GetProjectAiUsageTimeSery](../../models/operations/getprojectaiusagetimesery.md)[]         | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `customers`                                                                                            | [operations.Customer](../../models/operations/customer.md)[]                                           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `models`                                                                                               | [operations.GetProjectAiUsageModel](../../models/operations/getprojectaiusagemodel.md)[]               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `customersTruncated`                                                                                   | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `modelsTruncated`                                                                                      | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |