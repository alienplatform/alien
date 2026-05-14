# DataDeploymentRedeployRequested

## Example Usage

```typescript
import { DataDeploymentRedeployRequested } from "@alienplatform/platform-api/models";

let value: DataDeploymentRedeployRequested = {
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