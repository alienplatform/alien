# DataCollectionIssue1

## Example Usage

```typescript
import { DataCollectionIssue1 } from "@alienplatform/platform-api/models";

let value: DataCollectionIssue1 = {
  message: "<value>",
  reason: "forbidden",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `message`                                              | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `reason`                                               | [models.DataReason1](../models/datareason1.md)         | :heavy_check_mark:                                     | N/A                                                    |
| `severity`                                             | [models.StatusSeverity1](../models/statusseverity1.md) | :heavy_check_mark:                                     | N/A                                                    |
| `source`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |