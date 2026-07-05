# DataCollectionIssue14

## Example Usage

```typescript
import { DataCollectionIssue14 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue14 = {
  message: "<value>",
  reason: "forbidden",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason14](../models/datareason14.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity14](../models/statusseverity14.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |