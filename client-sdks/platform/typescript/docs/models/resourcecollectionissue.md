# ResourceCollectionIssue

## Example Usage

```typescript
import { ResourceCollectionIssue } from "@alienplatform/platform-api/models";

let value: ResourceCollectionIssue = {
  message: "<value>",
  reason: "not-installed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `message`                                                | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `reason`                                                 | [models.ResourceReason](../models/resourcereason.md)     | :heavy_check_mark:                                       | N/A                                                      |
| `severity`                                               | [models.ResourceSeverity](../models/resourceseverity.md) | :heavy_check_mark:                                       | N/A                                                      |
| `source`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |