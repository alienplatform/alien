# GetResourceDeploymentDetailDataStatus23

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus23 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus23 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "deleting",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue23](../../models/operations/collectionissue23.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health23](../../models/operations/health23.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle23](../../models/operations/lifecycle23.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |