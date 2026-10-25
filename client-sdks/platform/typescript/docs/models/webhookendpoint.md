# WebhookEndpoint

## Example Usage

```typescript
import { WebhookEndpoint } from "@alienplatform/platform-api/models";

let value: WebhookEndpoint = {
  url: "https://stiff-retention.info",
  description:
    "foodstuffs unless ouch incidentally gracefully stitcher obtrude phew",
  filterTypes: [
    "deployment.failed",
  ],
  disabled: false,
  id: "<id>",
  projectId: "<id>",
  createdAt: new Date("2025-10-28T20:13:58.027Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `url`                                                                                         | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `filterTypes`                                                                                 | [models.FilterType](../models/filtertype.md)[]                                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `disabled`                                                                                    | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |