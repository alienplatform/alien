# DeploymentStatePreparedStackApprovalPlugins

## Example Usage

```typescript
import { DeploymentStatePreparedStackApprovalPlugins } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackApprovalPlugins = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.DeploymentStatePreparedStackPluginsDecision](../models/deploymentstatepreparedstackpluginsdecision.md)       | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |