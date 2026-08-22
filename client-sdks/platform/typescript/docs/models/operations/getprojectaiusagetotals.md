# GetProjectAiUsageTotals

## Example Usage

```typescript
import { GetProjectAiUsageTotals } from "@alienplatform/platform-api/models/operations";

let value: GetProjectAiUsageTotals = {
  requests: 430337,
  successfulRequests: 386393,
  errorRequests: 135983,
  averageLatencyMs: 2507.1,
  p95LatencyMs: 2435.72,
  inputTokens: 634118,
  outputTokens: 324859,
  estimatedCostMicrousd: 466326,
  pricedRequests: 561781,
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