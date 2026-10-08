# GetResourceDeploymentDetailDataStatus21

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus21 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus21 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "failed",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue21](../../models/operations/collectionissue21.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health21](../../models/operations/health21.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle21](../../models/operations/lifecycle21.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |