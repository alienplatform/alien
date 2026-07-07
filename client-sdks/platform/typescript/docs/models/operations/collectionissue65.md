# CollectionIssue65

## Example Usage

```typescript
import { CollectionIssue65 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue65 = {
  message: "<value>",
  reason: "forbidden",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `reason`                                                                                     | [operations.Reason65](../../models/operations/reason65.md)                                   | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `severity`                                                                                   | [operations.CollectionIssueSeverity65](../../models/operations/collectionissueseverity65.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `source`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |