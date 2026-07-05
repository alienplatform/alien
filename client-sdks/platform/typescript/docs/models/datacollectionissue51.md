# DataCollectionIssue51

## Example Usage

```typescript
import { DataCollectionIssue51 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue51 = {
  message: "<value>",
  reason: "forbidden",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.DataReason51](../models/datareason51.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.StatusSeverity51](../models/statusseverity51.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |