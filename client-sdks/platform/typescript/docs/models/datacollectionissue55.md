# DataCollectionIssue55

## Example Usage

```typescript
import { DataCollectionIssue55 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue55 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason55](../models/datareason55.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity55](../models/statusseverity55.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |