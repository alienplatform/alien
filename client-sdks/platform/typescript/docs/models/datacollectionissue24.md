# DataCollectionIssue24

## Example Usage

```typescript
import { DataCollectionIssue24 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue24 = {
  message: "<value>",
  reason: "not-installed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason24](../models/datareason24.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity24](../models/statusseverity24.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |