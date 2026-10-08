# DeploymentStatePendingPreparedStackApprovalCustom

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackApprovalCustom } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackApprovalCustom = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                                 | [models.DeploymentStatePendingPreparedStackCustomDecision](../models/deploymentstatependingpreparedstackcustomdecision.md) | :heavy_check_mark:                                                                                                         | Whether matching operations run without approval.                                                                          |
| `maxRisk`                                                                                                                  | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.   |