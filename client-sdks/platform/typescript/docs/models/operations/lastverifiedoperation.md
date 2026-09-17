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
  tier: "mutating",
  commandState: "SUCCEEDED",
  verificationState: "verified",
  createdAt: new Date("2026-02-01T10:40:34.551Z"),
  updatedAt: new Date("2024-02-03T20:19:34.667Z"),
  verification: {
    state: "skipped",
    attempts: 455281,
    maxAttempts: 907051,
    deadline: new Date("2025-08-07T07:04:22.954Z"),
    reason: "<value>",
  },
  sensitiveOutput: {
    kind: "none",
  },
  resultAvailable: true,
  completedAt: new Date("2025-08-11T16:10:17.303Z"),
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