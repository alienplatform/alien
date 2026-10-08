# DeploymentDetailResponsePreparedStackApprovalCustom

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackApprovalCustom } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackApprovalCustom = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `decision`                                                                                                                     | [models.DeploymentDetailResponsePreparedStackCustomDecision](../models/deploymentdetailresponsepreparedstackcustomdecision.md) | :heavy_check_mark:                                                                                                             | Whether matching operations run without approval.                                                                              |
| `maxRisk`                                                                                                                      | *string*                                                                                                                       | :heavy_minus_sign:                                                                                                             | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.       |