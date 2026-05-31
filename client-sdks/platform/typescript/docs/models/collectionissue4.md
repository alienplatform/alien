# CollectionIssue4

## Example Usage

```typescript
import { CollectionIssue4 } from "@alienplatform/platform-api/models";

let value: CollectionIssue4 = {
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
| `reason`                                                                 | [models.Reason4](../models/reason4.md)                                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `severity`                                                               | [models.CollectionIssueSeverity4](../models/collectionissueseverity4.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `source`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |