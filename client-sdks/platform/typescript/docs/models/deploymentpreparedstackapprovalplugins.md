# DeploymentPreparedStackApprovalPlugins

## Example Usage

```typescript
import { DeploymentPreparedStackApprovalPlugins } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackApprovalPlugins = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.DeploymentPreparedStackPluginsDecision](../models/deploymentpreparedstackpluginsdecision.md)                 | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |