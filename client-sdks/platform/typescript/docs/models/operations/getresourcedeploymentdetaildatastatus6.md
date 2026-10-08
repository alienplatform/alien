# GetResourceDeploymentDetailDataStatus6

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus6 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus6 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "updating",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue6](../../models/operations/collectionissue6.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health6](../../models/operations/health6.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle6](../../models/operations/lifecycle6.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |