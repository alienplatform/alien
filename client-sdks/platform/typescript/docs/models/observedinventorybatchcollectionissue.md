# ObservedInventoryBatchCollectionIssue

## Example Usage

```typescript
import { ObservedInventoryBatchCollectionIssue } from "@alienplatform/platform-api/models";

let value: ObservedInventoryBatchCollectionIssue = {
  message: "<value>",
  reason: "api-unavailable",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `message`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `reason`                                                                             | [models.ObservedInventoryBatchReason](../models/observedinventorybatchreason.md)     | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `severity`                                                                           | [models.ObservedInventoryBatchSeverity](../models/observedinventorybatchseverity.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `source`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |