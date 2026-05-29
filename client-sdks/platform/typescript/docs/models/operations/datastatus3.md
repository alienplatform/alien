# DataStatus3

## Example Usage

```typescript
import { DataStatus3 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus3 = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "scaling",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue3](../../models/operations/collectionissue3.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health3](../../models/operations/health3.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle3](../../models/operations/lifecycle3.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |