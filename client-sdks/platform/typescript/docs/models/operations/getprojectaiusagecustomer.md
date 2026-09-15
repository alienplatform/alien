# GetProjectAiUsageCustomer

## Example Usage

```typescript
import { GetProjectAiUsageCustomer } from "@alienplatform/platform-api/models/operations";

let value: GetProjectAiUsageCustomer = {
  requests: 318650,
  successfulRequests: 236201,
  errorRequests: 627144,
  averageLatencyMs: 5220.13,
  p95LatencyMs: null,
  inputTokens: null,
  outputTokens: 482759,
  estimatedCostMicrousd: 139552,
  pricedRequests: 172932,
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