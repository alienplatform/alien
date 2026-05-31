# CollectionIssue9

## Example Usage

```typescript
import { CollectionIssue9 } from "@alienplatform/platform-api/models";

let value: CollectionIssue9 = {
  message: "<value>",
  reason: "forbidden",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `message`                                                                | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `reason`                                                                 | [models.Reason9](../models/reason9.md)                                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `severity`                                                               | [models.CollectionIssueSeverity9](../models/collectionissueseverity9.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `source`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |