# DataStatus22

## Example Usage

```typescript
import { DataStatus22 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus22 = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "updating",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue22](../../models/operations/collectionissue22.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health22](../../models/operations/health22.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle22](../../models/operations/lifecycle22.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |