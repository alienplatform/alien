# CollectionIssue64

## Example Usage

```typescript
import { CollectionIssue64 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue64 = {
  message: "<value>",
  reason: "timed-out",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `reason`                                                                                     | [operations.Reason64](../../models/operations/reason64.md)                                   | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `severity`                                                                                   | [operations.CollectionIssueSeverity64](../../models/operations/collectionissueseverity64.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `source`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |