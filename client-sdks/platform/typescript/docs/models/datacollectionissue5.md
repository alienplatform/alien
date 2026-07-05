# DataCollectionIssue5

## Example Usage

```typescript
import { DataCollectionIssue5 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue5 = {
  message: "<value>",
  reason: "not-installed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `message`                                              | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `reason`                                               | [models.DataReason5](../models/datareason5.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity5](../models/statusseverity5.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |