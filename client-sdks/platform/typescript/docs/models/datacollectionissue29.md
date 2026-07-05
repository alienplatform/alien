# DataCollectionIssue29

## Example Usage

```typescript
import { DataCollectionIssue29 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue29 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason29](../models/datareason29.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity29](../models/statusseverity29.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |