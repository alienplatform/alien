# Selected

## Example Usage

```typescript
import { Selected } from "@alienplatform/platform-api/models/operations";

let value: Selected = {
  name: "<value>",
  version: "<value>",
  source: "custom",
  operations: [
    {
      name: "<value>",
      tier: "read-only",
    },
  ],
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                                       | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `version`                                                                                                                    | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `source`                                                                                                                     | [operations.GetRemoteOperatorProjectSummarySource](../../models/operations/getremoteoperatorprojectsummarysource.md)         | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `operations`                                                                                                                 | [operations.GetRemoteOperatorProjectSummaryOperation](../../models/operations/getremoteoperatorprojectsummaryoperation.md)[] | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |