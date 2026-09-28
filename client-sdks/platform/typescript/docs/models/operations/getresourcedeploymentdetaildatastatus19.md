# GetResourceDeploymentDetailDataStatus19

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus19 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus19 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "updating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue19](../../models/operations/collectionissue19.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health19](../../models/operations/health19.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle19](../../models/operations/lifecycle19.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |