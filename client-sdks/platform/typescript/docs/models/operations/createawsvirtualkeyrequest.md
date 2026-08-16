# CreateAwsVirtualKeyRequest

## Example Usage

```typescript
import { CreateAwsVirtualKeyRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateAwsVirtualKeyRequest = {
  selector: {
    externalId: "ext_example_01",
  },
  awsAccountId: "<id>",
  awsRegion: "<value>",
  idempotencyKey: "<value>",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `project`                     | *string*                      | :heavy_minus_sign:            | Filter by project ID or name. |
| `selector`                    | *operations.SelectorUnion*    | :heavy_check_mark:            | N/A                           |
| `awsAccountId`                | *string*                      | :heavy_check_mark:            | N/A                           |
| `awsRegion`                   | *string*                      | :heavy_check_mark:            | N/A                           |
| `idempotencyKey`              | *string*                      | :heavy_check_mark:            | N/A                           |
| `alias`                       | *string*                      | :heavy_minus_sign:            | N/A                           |
| `description`                 | *string*                      | :heavy_minus_sign:            | N/A                           |
| `deletionWindowDays`          | *number*                      | :heavy_minus_sign:            | N/A                           |
| `tags`                        | Record<string, *string*>      | :heavy_minus_sign:            | N/A                           |