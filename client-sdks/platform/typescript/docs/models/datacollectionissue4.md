# DataCollectionIssue4

## Example Usage

```typescript
import { DataCollectionIssue4 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue4 = {
  message: "<value>",
  reason: "forbidden",
  severity: "warning",
  source: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `message`                                              | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `reason`                                               | [models.DataReason4](../models/datareason4.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity4](../models/statusseverity4.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |