# GetProjectEncryptionUsageOperation

## Example Usage

```typescript
import { GetProjectEncryptionUsageOperation } from "@alienplatform/platform-api/models/operations";

let value: GetProjectEncryptionUsageOperation = {
  requests: 118075,
  successfulRequests: 457370,
  errorRequests: 203156,
  averageLatencyMs: 2936.66,
  p95LatencyMs: 7553.43,
  operation: "encrypt",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `requests`                                                           | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `successfulRequests`                                                 | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `errorRequests`                                                      | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `averageLatencyMs`                                                   | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `p95LatencyMs`                                                       | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `operation`                                                          | [operations.OperationEnum](../../models/operations/operationenum.md) | :heavy_check_mark:                                                   | N/A                                                                  |