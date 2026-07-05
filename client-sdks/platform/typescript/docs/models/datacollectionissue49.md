# DataCollectionIssue49

## Example Usage

```typescript
import { DataCollectionIssue49 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue49 = {
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
| `reason`                                                 | [models.DataReason49](../models/datareason49.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity49](../models/statusseverity49.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |