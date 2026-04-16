# Http

## Example Usage

```typescript
import { Http } from "@alienplatform/platform-api/models/operations";

let value: Http = {};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `inFlightRequests`                                                 | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `totalRequests`                                                    | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `latencyP95Ms`                                                     | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `latencyP99Ms`                                                     | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status2xx`                                                        | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status4xx`                                                        | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status5xx`                                                        | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `topEndpoints`                                                     | [operations.TopEndpoint](../../models/operations/topendpoint.md)[] | :heavy_minus_sign:                                                 | N/A                                                                |