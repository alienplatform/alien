# CollectionIssue27

## Example Usage

```typescript
import { CollectionIssue27 } from "@alienplatform/platform-api/models";

let value: CollectionIssue27 = {
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
| `reason`                                                                   | [models.Reason27](../models/reason27.md)                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `severity`                                                                 | [models.CollectionIssueSeverity27](../models/collectionissueseverity27.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `source`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |