# CollectionIssue8

## Example Usage

```typescript
import { CollectionIssue8 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue8 = {
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
| `reason`                                                                                   | [operations.Reason8](../../models/operations/reason8.md)                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `severity`                                                                                 | [operations.CollectionIssueSeverity8](../../models/operations/collectionissueseverity8.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `source`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |