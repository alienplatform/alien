# DataStatus33

## Example Usage

```typescript
import { DataStatus33 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus33 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "stopping",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue33](../../models/operations/collectionissue33.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health33](../../models/operations/health33.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle33](../../models/operations/lifecycle33.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |