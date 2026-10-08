# ObservedInventoryBatchRaw

## Example Usage

```typescript
import { ObservedInventoryBatchRaw } from "@alienplatform/platform-api/models";

let value: ObservedInventoryBatchRaw = {
  body: "<value>",
  collectedAt: new Date("2025-02-25T23:01:55.621Z"),
  format: "text",
  source: "<value>",
  truncated: true,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `body`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `collectedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `format`                                                                                      | [models.ObservedInventoryBatchFormat](../models/observedinventorybatchformat.md)              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `truncated`                                                                                   | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |