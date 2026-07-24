# EventListItemResponseDataDeploymentRedeployRequested

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentRedeployRequested } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentRedeployRequested = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRedeployRequested",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `deploymentId`                     | *string*                           | :heavy_check_mark:                 | ID of the deployment               |
| `releaseId`                        | *string*                           | :heavy_check_mark:                 | ID of the release being redeployed |
| `type`                             | *"DeploymentRedeployRequested"*    | :heavy_check_mark:                 | N/A                                |
