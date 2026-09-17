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
  createdAt: new Date("2026-12-07T17:23:13.325Z"),
  updatedAt: new Date("2025-12-03T07:47:55.849Z"),
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