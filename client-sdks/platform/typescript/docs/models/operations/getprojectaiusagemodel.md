# GetProjectAiUsageModel

## Example Usage

```typescript
import { GetProjectAiUsageModel } from "@alienplatform/platform-api/models/operations";

let value: GetProjectAiUsageModel = {
  requests: 524841,
  successfulRequests: 429001,
  errorRequests: 284409,
  averageLatencyMs: 5776.86,
  p95LatencyMs: 7236.06,
  inputTokens: null,
  outputTokens: 480329,
  estimatedCostMicrousd: 309767,
  pricedRequests: 188816,
  publicModel: "<value>",
  provider: "<value>",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `requests`              | *number*                | :heavy_check_mark:      | N/A                     |
| `successfulRequests`    | *number*                | :heavy_check_mark:      | N/A                     |
| `errorRequests`         | *number*                | :heavy_check_mark:      | N/A                     |
| `averageLatencyMs`      | *number*                | :heavy_check_mark:      | N/A                     |
| `p95LatencyMs`          | *number*                | :heavy_check_mark:      | N/A                     |
| `inputTokens`           | *number*                | :heavy_check_mark:      | N/A                     |
| `outputTokens`          | *number*                | :heavy_check_mark:      | N/A                     |
| `estimatedCostMicrousd` | *number*                | :heavy_check_mark:      | N/A                     |
| `pricedRequests`        | *number*                | :heavy_check_mark:      | N/A                     |
| `publicModel`           | *string*                | :heavy_check_mark:      | N/A                     |
| `provider`              | *string*                | :heavy_check_mark:      | N/A                     |