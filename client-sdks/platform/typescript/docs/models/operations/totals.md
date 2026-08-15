# Totals

## Example Usage

```typescript
import { Totals } from "@alienplatform/platform-api/models/operations";

let value: Totals = {
  requests: 299586,
  successfulRequests: 873533,
  errorRequests: 232940,
  averageLatencyMs: null,
  p95LatencyMs: 5533.3,
  inputTokens: 818361,
  outputTokens: null,
  estimatedCostMicrousd: 744870,
  pricedRequests: 317021,
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