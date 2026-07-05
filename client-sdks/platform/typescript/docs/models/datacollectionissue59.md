# DataCollectionIssue59

## Example Usage

```typescript
import { DataCollectionIssue59 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue59 = {
  message: "<value>",
  reason: "timed-out",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason59](../models/datareason59.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity59](../models/statusseverity59.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |