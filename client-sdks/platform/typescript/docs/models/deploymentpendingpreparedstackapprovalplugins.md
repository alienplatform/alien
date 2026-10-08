# DeploymentPendingPreparedStackApprovalPlugins

## Example Usage

```typescript
import { DeploymentPendingPreparedStackApprovalPlugins } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackApprovalPlugins = {
  decision: "manual",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.DeploymentPendingPreparedStackPluginsDecision](../models/deploymentpendingpreparedstackpluginsdecision.md)   | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |