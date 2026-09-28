# GetResourceDeploymentDetailDataStatus73

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus73 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus73 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "scaling",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue70](../../models/operations/collectionissue70.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health73](../../models/operations/health73.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle73](../../models/operations/lifecycle73.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |