# CollectionIssue13

## Example Usage

```typescript
import { CollectionIssue13 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue13 = {
  message: "<value>",
  reason: "not-installed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `reason`                                                                                     | [operations.Reason13](../../models/operations/reason13.md)                                   | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `severity`                                                                                   | [operations.CollectionIssueSeverity13](../../models/operations/collectionissueseverity13.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `source`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |