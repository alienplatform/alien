# DataCollectionIssue30

## Example Usage

```typescript
import { DataCollectionIssue30 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue30 = {
  message: "<value>",
  reason: "timed-out",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason30](../models/datareason30.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity30](../models/statusseverity30.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |