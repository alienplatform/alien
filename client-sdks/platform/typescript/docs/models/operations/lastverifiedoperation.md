# LastVerifiedOperation

## Example Usage

```typescript
import { LastVerifiedOperation } from "@alienplatform/platform-api/models/operations";

let value: LastVerifiedOperation = {
  invocationId: "<id>",
  deploymentId: "<id>",
  commandId: "<id>",
  plugin: "<value>",
  pluginVersion: "<value>",
  operation: "<value>",
  tier: "destructive",
  commandState: "SUCCEEDED",
  verificationState: "not-required",
  createdAt: new Date("2026-01-08T02:31:21.611Z"),
  updatedAt: new Date("2025-05-13T23:43:15.835Z"),
  verification: {
    state: "skipped",
    attempts: 371867,
    maxAttempts: 533116,
    deadline: new Date("2024-06-09T09:14:41.952Z"),
    reason: "<value>",
  },
  sensitiveOutput: {
    kind: "requireConfirmation",
  },
  resultAvailable: false,
  completedAt: new Date("2024-12-21T05:10:16.528Z"),
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `invocationId`                                                                                                                   | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `deploymentId`                                                                                                                   | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `commandId`                                                                                                                      | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `plugin`                                                                                                                         | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `pluginVersion`                                                                                                                  | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `operation`                                                                                                                      | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `tier`                                                                                                                           | [operations.TierSucceeded](../../models/operations/tiersucceeded.md)                                                             | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `commandState`                                                                                                                   | [operations.CommandState](../../models/operations/commandstate.md)                                                               | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `verificationState`                                                                                                              | [operations.VerificationStateSucceeded](../../models/operations/verificationstatesucceeded.md)                                   | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `createdAt`                                                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                    | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `updatedAt`                                                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                    | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `verification`                                                                                                                   | [operations.GetRemoteOperatorProjectSummaryVerification](../../models/operations/getremoteoperatorprojectsummaryverification.md) | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `sensitiveOutput`                                                                                                                | *operations.SensitiveOutput*                                                                                                     | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `resultAvailable`                                                                                                                | *boolean*                                                                                                                        | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `result`                                                                                                                         | *any*                                                                                                                            | :heavy_minus_sign:                                                                                                               | N/A                                                                                                                              |
| `completedAt`                                                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                    | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |