# GetResourceDeploymentDetailDataStatus4

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus4 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus4 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue4](../../models/operations/collectionissue4.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health4](../../models/operations/health4.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle4](../../models/operations/lifecycle4.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |