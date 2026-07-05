# DataCollectionIssue22

## Example Usage

```typescript
import { DataCollectionIssue22 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue22 = {
  message: "<value>",
  reason: "timed-out",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason22](../models/datareason22.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity22](../models/statusseverity22.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |