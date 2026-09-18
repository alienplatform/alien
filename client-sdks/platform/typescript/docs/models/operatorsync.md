# OperatorSync

Plugin/operations-bundle sync status for this deployment's Operator. Null for non-pull deployments.

## Example Usage

```typescript
import { OperatorSync } from "@alienplatform/platform-api/models";

let value: OperatorSync = {
  id: "dosy_uxveigz3biyrosqycaqubbr",
  status: "stuck",
  targetBundleHash: "<value>",
  targetSetAt: new Date("2024-07-10T07:09:53.767Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment operator sync.                                           | dosy_uxveigz3biyrosqycaqubbr                                                                  |
| `status`                                                                                      | [models.DeploymentOperatorSyncStatus](../models/deploymentoperatorsyncstatus.md)              | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `stuckReason`                                                                                 | [models.DeploymentOperatorSyncStuckReason](../models/deploymentoperatorsyncstuckreason.md)    | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `statusMessage`                                                                               | *string*                                                                                      | :heavy_minus_sign:                                                                            | Human-readable explanation of the status                                                      |                                                                                               |
| `targetBundleHash`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `observedBundleHash`                                                                          | *string*                                                                                      | :heavy_minus_sign:                                                                            | Enabled-plugin-bundle-set hash last reported by the Operator                                  |                                                                                               |
| `missingOperations`                                                                           | *string*[]                                                                                    | :heavy_minus_sign:                                                                            | Expected `plugin/operation` names not reported by the Operator (catalog-mismatch only)        |                                                                                               |
| `targetSetAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |