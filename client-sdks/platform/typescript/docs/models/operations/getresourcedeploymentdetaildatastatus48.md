# GetResourceDeploymentDetailDataStatus48

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus48 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus48 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "scaling",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue48](../../models/operations/collectionissue48.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health48](../../models/operations/health48.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle48](../../models/operations/lifecycle48.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |