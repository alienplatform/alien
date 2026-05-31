# CollectionIssue24

## Example Usage

```typescript
import { CollectionIssue24 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue24 = {
  message: "<value>",
  reason: "not-installed",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `reason`                                                                                     | [operations.Reason24](../../models/operations/reason24.md)                                   | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `severity`                                                                                   | [operations.CollectionIssueSeverity24](../../models/operations/collectionissueseverity24.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `source`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |