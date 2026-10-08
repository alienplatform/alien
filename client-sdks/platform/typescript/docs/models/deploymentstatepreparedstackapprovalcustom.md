# DeploymentStatePreparedStackApprovalCustom

## Example Usage

```typescript
import { DeploymentStatePreparedStackApprovalCustom } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackApprovalCustom = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.DeploymentStatePreparedStackCustomDecision](../models/deploymentstatepreparedstackcustomdecision.md)         | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |