# ResourceHeartbeatRaw

## Example Usage

```typescript
import { ResourceHeartbeatRaw } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatRaw = {
  body: "<value>",
  collectedAt: new Date("2025-02-16T21:57:38.321Z"),
  format: "text",
  source: "<value>",
  truncated: false,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `body`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `collectedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `format`                                                                                      | [models.ResourceHeartbeatFormat](../models/resourceheartbeatformat.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `truncated`                                                                                   | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |