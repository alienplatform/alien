# CollectionIssue4

## Example Usage

```typescript
import { CollectionIssue4 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue4 = {
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
| `reason`                                                                                   | [operations.Reason4](../../models/operations/reason4.md)                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `severity`                                                                                 | [operations.CollectionIssueSeverity4](../../models/operations/collectionissueseverity4.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `source`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |