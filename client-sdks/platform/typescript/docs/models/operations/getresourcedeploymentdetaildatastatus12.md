# GetResourceDeploymentDetailDataStatus12

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus12 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus12 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "stopped",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue12](../../models/operations/collectionissue12.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health12](../../models/operations/health12.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle12](../../models/operations/lifecycle12.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |