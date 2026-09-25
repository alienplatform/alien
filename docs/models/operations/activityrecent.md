# ActivityRecent

## Example Usage

```typescript
import { ActivityRecent } from "@alienplatform/platform-api/models/operations";

let value: ActivityRecent = {
  invocationId: "<id>",
  deploymentId: "<id>",
  commandId: "<id>",
  plugin: "<value>",
  pluginVersion: "<value>",
  operation: "<value>",
  tier: "destructive",
  commandState: "<value>",
  verificationState: "failed",
  createdAt: new Date("2026-08-28T13:55:38.307Z"),
  updatedAt: new Date("2024-04-29T15:28:43.378Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `invocationId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `commandId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `plugin`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `pluginVersion`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `operation`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `tier`                                                                                        | [operations.RecentTier](../../models/operations/recenttier.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `commandState`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `verificationState`                                                                           | [operations.RecentVerificationState](../../models/operations/recentverificationstate.md)      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |