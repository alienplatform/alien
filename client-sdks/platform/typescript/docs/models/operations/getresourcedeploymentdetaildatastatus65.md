# GetResourceDeploymentDetailDataStatus65

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataStatus65 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataStatus65 = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue65](../../models/operations/collectionissue65.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health65](../../models/operations/health65.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle65](../../models/operations/lifecycle65.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |