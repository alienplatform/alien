# DataCollectionIssue7

## Example Usage

```typescript
import { DataCollectionIssue7 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue7 = {
  message: "<value>",
  reason: "api-unavailable",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `message`                                              | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `reason`                                               | [models.DataReason7](../models/datareason7.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity7](../models/statusseverity7.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |