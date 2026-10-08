# GetResourceDeploymentDetailDataStatus45

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus45 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus45 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "stopped",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue45](../../models/operations/collectionissue45.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health45](../../models/operations/health45.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle45](../../models/operations/lifecycle45.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |