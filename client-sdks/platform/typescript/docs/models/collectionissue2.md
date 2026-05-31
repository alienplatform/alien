# CollectionIssue2

## Example Usage

```typescript
import { CollectionIssue2 } from "@alienplatform/platform-api/models";

let value: CollectionIssue2 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `message`                                                                | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `reason`                                                                 | [models.Reason2](../models/reason2.md)                                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `severity`                                                               | [models.CollectionIssueSeverity2](../models/collectionissueseverity2.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `source`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |