# DataStatus74

## Example Usage

```typescript
import { DataStatus74 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus74 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "deleted",
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