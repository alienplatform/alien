# GetResourceDeploymentDetailDataStatus35

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus35 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus35 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue35](../../models/operations/collectionissue35.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health35](../../models/operations/health35.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle35](../../models/operations/lifecycle35.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |