# CollectionIssue20

## Example Usage

```typescript
import { CollectionIssue20 } from "@alienplatform/platform-api/models";

let value: CollectionIssue20 = {
  message: "<value>",
  reason: "api-unavailable",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `message`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `reason`                                                                   | [models.Reason20](../models/reason20.md)                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `severity`                                                                 | [models.CollectionIssueSeverity20](../models/collectionissueseverity20.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `source`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |