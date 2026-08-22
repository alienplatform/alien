# GetProjectEncryptionUsageTimeSery

## Example Usage

```typescript
import { GetProjectEncryptionUsageTimeSery } from "@alienplatform/platform-api/models/operations";

let value: GetProjectEncryptionUsageTimeSery = {
  requests: 304587,
  successfulRequests: 858401,
  errorRequests: 554545,
  averageLatencyMs: null,
  p95LatencyMs: 9863.66,
  bucket: new Date("2025-01-24T12:06:49.485Z"),
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
| `bucket`                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |