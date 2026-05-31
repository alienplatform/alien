# CollectionIssue18

## Example Usage

```typescript
import { CollectionIssue18 } from "@alienplatform/platform-api/models";

let value: CollectionIssue18 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `message`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `reason`                                                                   | [models.Reason18](../models/reason18.md)                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `severity`                                                                 | [models.CollectionIssueSeverity18](../models/collectionissueseverity18.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `source`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |