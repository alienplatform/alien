# GetProjectEncryptionUsageTotals

## Example Usage

```typescript
import { GetProjectEncryptionUsageTotals } from "@alienplatform/platform-api/models/operations";

let value: GetProjectEncryptionUsageTotals = {
  requests: 6571,
  successfulRequests: 366488,
  errorRequests: 223047,
  averageLatencyMs: 3122.74,
  p95LatencyMs: 8636.11,
};
```

## Fields

| Field                | Type                 | Required             | Description          |
| -------------------- | -------------------- | -------------------- | -------------------- |
| `requests`           | *number*             | :heavy_check_mark:   | N/A                  |
| `successfulRequests` | *number*             | :heavy_check_mark:   | N/A                  |
| `errorRequests`      | *number*             | :heavy_check_mark:   | N/A                  |
| `averageLatencyMs`   | *number*             | :heavy_check_mark:   | N/A                  |
| `p95LatencyMs`       | *number*             | :heavy_check_mark:   | N/A                  |