# CollectionIssue2

## Example Usage

```typescript
import { CollectionIssue2 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue2 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `message`                                                                                  | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `reason`                                                                                   | [operations.Reason2](../../models/operations/reason2.md)                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `severity`                                                                                 | [operations.CollectionIssueSeverity2](../../models/operations/collectionissueseverity2.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `source`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |