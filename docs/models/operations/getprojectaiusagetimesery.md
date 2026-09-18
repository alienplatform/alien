# GetProjectAiUsageTimeSery

## Example Usage

```typescript
import { GetProjectAiUsageTimeSery } from "@alienplatform/platform-api/models/operations";

let value: GetProjectAiUsageTimeSery = {
  requests: 481153,
  successfulRequests: 820059,
  errorRequests: 70173,
  averageLatencyMs: 8643.1,
  p95LatencyMs: 2585.15,
  inputTokens: 37730,
  outputTokens: 551211,
  estimatedCostMicrousd: 814560,
  pricedRequests: 883256,
  bucket: new Date("2026-06-18T09:31:16.849Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `requests`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `successfulRequests`                                                                          | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errorRequests`                                                                               | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `averageLatencyMs`                                                                            | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `p95LatencyMs`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `inputTokens`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `outputTokens`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `estimatedCostMicrousd`                                                                       | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `pricedRequests`                                                                              | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `bucket`                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |