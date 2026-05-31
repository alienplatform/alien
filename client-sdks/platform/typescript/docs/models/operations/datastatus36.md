# DataStatus36

## Example Usage

```typescript
import { DataStatus36 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus36 = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "creating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue36](../../models/operations/collectionissue36.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health36](../../models/operations/health36.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle36](../../models/operations/lifecycle36.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |