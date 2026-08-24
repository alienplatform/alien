# Operation

## Example Usage

```typescript
import { Operation } from "@alienplatform/platform-api/models/operations";

let value: Operation = {
  requests: 622966,
  successfulRequests: 168801,
  errorRequests: 137101,
  averageLatencyMs: 81.58,
  p95LatencyMs: 7968.31,
  operation: "decrypt",
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