# CollectionIssue22

## Example Usage

```typescript
import { CollectionIssue22 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue22 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `reason`                                                                                     | [operations.Reason22](../../models/operations/reason22.md)                                   | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `severity`                                                                                   | [operations.CollectionIssueSeverity22](../../models/operations/collectionissueseverity22.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `source`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |