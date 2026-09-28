# GetResourceDeploymentDetailDataStatus20

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus20 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus20 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "stopped",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue20](../../models/operations/collectionissue20.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health20](../../models/operations/health20.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle20](../../models/operations/lifecycle20.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |