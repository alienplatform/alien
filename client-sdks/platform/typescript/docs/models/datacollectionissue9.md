# DataCollectionIssue9

## Example Usage

```typescript
import { DataCollectionIssue9 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue9 = {
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
| `reason`                                               | [models.DataReason9](../models/datareason9.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity9](../models/statusseverity9.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |