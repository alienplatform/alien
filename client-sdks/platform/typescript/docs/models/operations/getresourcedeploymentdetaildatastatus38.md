# GetResourceDeploymentDetailDataStatus38

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus38 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus38 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "running",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue38](../../models/operations/collectionissue38.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health38](../../models/operations/health38.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle38](../../models/operations/lifecycle38.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |