# DataCollectionIssue27

## Example Usage

```typescript
import { DataCollectionIssue27 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue27 = {
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
| `reason`                                                 | [models.DataReason27](../models/datareason27.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity27](../models/statusseverity27.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |