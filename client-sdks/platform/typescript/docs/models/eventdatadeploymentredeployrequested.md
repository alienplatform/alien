# EventDataDeploymentRedeployRequested

## Example Usage

```typescript
import { EventDataDeploymentRedeployRequested } from "@alienplatform/platform-api/models";

let value: EventDataDeploymentRedeployRequested = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRedeployRequested",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `actor`                            | *models.EventActorUnion4*          | :heavy_minus_sign:                 | N/A                                |
| `deploymentId`                     | *string*                           | :heavy_check_mark:                 | ID of the deployment               |
| `releaseId`                        | *string*                           | :heavy_check_mark:                 | ID of the release being redeployed |
| `type`                             | *"DeploymentRedeployRequested"*    | :heavy_check_mark:                 | N/A                                |