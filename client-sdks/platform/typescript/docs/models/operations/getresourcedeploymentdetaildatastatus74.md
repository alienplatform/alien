# GetResourceDeploymentDetailDataStatus74

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus74 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus74 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "scaling",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue71](../../models/operations/collectionissue71.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health74](../../models/operations/health74.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle74](../../models/operations/lifecycle74.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |