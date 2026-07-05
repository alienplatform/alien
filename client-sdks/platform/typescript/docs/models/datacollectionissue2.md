# DataCollectionIssue2

## Example Usage

```typescript
import { DataCollectionIssue2 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue2 = {
  message: "<value>",
  reason: "forbidden",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `message`                                              | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `reason`                                               | [models.DataReason2](../models/datareason2.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity2](../models/statusseverity2.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |