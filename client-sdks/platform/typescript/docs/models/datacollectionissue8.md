# DataCollectionIssue8

## Example Usage

```typescript
import { DataCollectionIssue8 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue8 = {
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
| `reason`                                               | [models.DataReason8](../models/datareason8.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity8](../models/statusseverity8.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |