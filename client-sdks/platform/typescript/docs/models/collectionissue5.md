# CollectionIssue5

## Example Usage

```typescript
import { CollectionIssue5 } from "@alienplatform/platform-api/models";

let value: CollectionIssue5 = {
  message: "<value>",
  reason: "timed-out",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `message`                                                                | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `reason`                                                                 | [models.Reason5](../models/reason5.md)                                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `severity`                                                               | [models.CollectionIssueSeverity5](../models/collectionissueseverity5.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `source`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |