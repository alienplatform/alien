# GetResourceDeploymentDetailDataStatus1

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus1 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus1 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "unknown",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue1](../../models/operations/collectionissue1.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health1](../../models/operations/health1.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle1](../../models/operations/lifecycle1.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |