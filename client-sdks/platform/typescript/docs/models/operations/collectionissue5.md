# CollectionIssue5

## Example Usage

```typescript
import { CollectionIssue5 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue5 = {
  message: "<value>",
  reason: "timed-out",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `message`                                                                                  | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `reason`                                                                                   | [operations.Reason5](../../models/operations/reason5.md)                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `severity`                                                                                 | [operations.CollectionIssueSeverity5](../../models/operations/collectionissueseverity5.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `source`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |