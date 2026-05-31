# Resource

## Example Usage

```typescript
import { Resource } from "@alienplatform/platform-api/models/operations";

let value: Resource = {
  resourceType: "<value>",
  resourceId: "<id>",
  name: "<value>",
  backend: "<value>",
  controllerPlatform: "<value>",
  health: "<value>",
  lifecycle: "<value>",
  message: "<value>",
  partial: false,
  providerStale: true,
  platformStale: false,
  desiredCount: null,
  currentCount: 757492,
  readyCount: 823541,
  deploymentCount: 818027,
  attentionCount: 625461,
  lastObservedAt: new Date("2026-04-22T17:13:39.553Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `resourceType`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `controllerPlatform`                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `health`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lifecycle`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `partial`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `desiredCount`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `currentCount`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `readyCount`                                                                                  | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentCount`                                                                             | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `attentionCount`                                                                              | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lastObservedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |