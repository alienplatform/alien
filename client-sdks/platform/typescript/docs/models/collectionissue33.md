# CollectionIssue33

## Example Usage

```typescript
import { CollectionIssue33 } from "@alienplatform/platform-api/models";

let value: CollectionIssue33 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `message`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `reason`                                                                   | [models.Reason33](../models/reason33.md)                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `severity`                                                                 | [models.CollectionIssueSeverity33](../models/collectionissueseverity33.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `source`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |