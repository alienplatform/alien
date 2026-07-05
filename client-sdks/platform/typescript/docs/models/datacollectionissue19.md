# DataCollectionIssue19

## Example Usage

```typescript
import { DataCollectionIssue19 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue19 = {
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
| `reason`                                                 | [models.DataReason19](../models/datareason19.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity19](../models/statusseverity19.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |