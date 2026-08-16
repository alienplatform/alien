# Available

## Example Usage

```typescript
import { Available } from "@alienplatform/platform-api/models/operations";

let value: Available = {
  status: "available",
  range: "7d",
  pricing: {
    label: "Estimated provider cost",
    coverage: 4485.61,
    revision: "<value>",
  },
  totals: {
    requests: 693783,
    successfulRequests: 310561,
    errorRequests: 203130,
    averageLatencyMs: 4118.4,
    p95LatencyMs: null,
    inputTokens: 639818,
    outputTokens: 264082,
    estimatedCostMicrousd: 970447,
    pricedRequests: 94285,
  },
  timeSeries: [
    {
      requests: 925877,
      successfulRequests: 276847,
      errorRequests: 426254,
      averageLatencyMs: null,
      p95LatencyMs: 5677.9,
      inputTokens: null,
      outputTokens: 937346,
      estimatedCostMicrousd: 165362,
      pricedRequests: 39907,
      bucket: new Date("2026-10-28T10:39:01.844Z"),
    },
  ],
  customers: [
    {
      requests: 283671,
      successfulRequests: 61648,
      errorRequests: 770565,
      averageLatencyMs: 8903.54,
      p95LatencyMs: 4971.72,
      inputTokens: 18051,
      outputTokens: 983025,
      estimatedCostMicrousd: 489322,
      pricedRequests: 284032,
      deploymentGroupId: "<id>",
      name: "<value>",
      externalId: "<id>",
    },
  ],
  models: [],
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `status`                                                                                 | *"available"*                                                                            | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `range`                                                                                  | [operations.RangeResponse](../../models/operations/rangeresponse.md)                     | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `pricing`                                                                                | [operations.Pricing](../../models/operations/pricing.md)                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `totals`                                                                                 | [operations.Totals](../../models/operations/totals.md)                                   | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `timeSeries`                                                                             | [operations.TimeSery](../../models/operations/timesery.md)[]                             | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `customers`                                                                              | [operations.Customer](../../models/operations/customer.md)[]                             | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `models`                                                                                 | [operations.GetProjectAiUsageModel](../../models/operations/getprojectaiusagemodel.md)[] | :heavy_check_mark:                                                                       | N/A                                                                                      |