# DataCollectionIssue66

## Example Usage

```typescript
import { DataCollectionIssue66 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue66 = {
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
| `reason`                                                 | [models.DataReason66](../models/datareason66.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity66](../models/statusseverity66.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |