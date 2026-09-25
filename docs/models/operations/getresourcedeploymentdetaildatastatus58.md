# GetResourceDeploymentDetailDataStatus58

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus58 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus58 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "deleting",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue58](../../models/operations/collectionissue58.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health58](../../models/operations/health58.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle58](../../models/operations/lifecycle58.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |