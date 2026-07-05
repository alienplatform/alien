# DataCollectionIssue13

## Example Usage

```typescript
import { DataCollectionIssue13 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue13 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason13](../models/datareason13.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity13](../models/statusseverity13.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |