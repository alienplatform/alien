# DataCollectionIssue45

## Example Usage

```typescript
import { DataCollectionIssue45 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue45 = {
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
| `reason`                                                 | [models.DataReason45](../models/datareason45.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity45](../models/statusseverity45.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |