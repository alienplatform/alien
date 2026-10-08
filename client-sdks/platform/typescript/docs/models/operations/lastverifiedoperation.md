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
  accessRequestId: null,
  commandState: "SUCCEEDED",
  verificationState: "failed",
  createdAt: new Date("2025-05-13T23:43:15.835Z"),
  updatedAt: new Date("2026-09-21T03:02:52.045Z"),
  verification: {
    state: "verified",
    attempts: 533116,
    maxAttempts: 249676,
    deadline: new Date("2025-03-26T23:32:49.615Z"),
    reason: "<value>",
  },
  sensitiveOutput: {
    kind: "redact",
    fields: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
  },
  resultAvailable: true,
  completedAt: new Date("2026-08-13T12:41:44.762Z"),
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
| `accessRequestId`                                                                                                                | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `commandState`                                                                                                                   | [operations.CommandState](../../models/operations/commandstate.md)                                                               | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `verificationState`                                                                                                              | [operations.VerificationStateSucceeded](../../models/operations/verificationstatesucceeded.md)                                   | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `createdAt`                                                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                    | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `updatedAt`                                                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                    | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `verification`                                                                                                                   | [operations.GetRemoteOperatorProjectSummaryVerification](../../models/operations/getremoteoperatorprojectsummaryverification.md) | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `sensitiveOutput`                                                                                                                | *operations.SensitiveOutput*                                                                                                     | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `resultAvailable`                                                                                                                | *boolean*                                                                                                                        | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `result`                                                                                                                         | *any*                                                                                                                            | :heavy_minus_sign:                                                                                                               | N/A                                                                                                                              |
| `completedAt`                                                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                    | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |