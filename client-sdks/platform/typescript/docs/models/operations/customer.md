# Customer

## Example Usage

```typescript
import { Customer } from "@alienplatform/platform-api/models/operations";

let value: Customer = {
  requests: 838630,
  successfulRequests: 5761,
  errorRequests: 743034,
  averageLatencyMs: 5696.82,
  p95LatencyMs: 7665.66,
  inputTokens: 736621,
  outputTokens: 995634,
  estimatedCostMicrousd: 492495,
  pricedRequests: 836432,
  deploymentGroupId: "<id>",
  name: "<value>",
  externalId: "<id>",
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
| `deploymentGroupId`     | *string*                | :heavy_check_mark:      | N/A                     |
| `name`                  | *string*                | :heavy_check_mark:      | N/A                     |
| `externalId`            | *string*                | :heavy_check_mark:      | N/A                     |