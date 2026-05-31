# CollectionIssue8

## Example Usage

```typescript
import { CollectionIssue8 } from "@alienplatform/platform-api/models";

let value: CollectionIssue8 = {
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
| `reason`                                                                 | [models.Reason8](../models/reason8.md)                                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `severity`                                                               | [models.CollectionIssueSeverity8](../models/collectionissueseverity8.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `source`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |