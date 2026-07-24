# EventListItemResponseDataDeploymentReleased

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentReleased } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentReleased = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentReleased",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `deploymentId`                                     | *string*                                           | :heavy_check_mark:                                 | ID of the deployment                               |
| `previousReleaseId`                                | *string*                                           | :heavy_minus_sign:                                 | ID of the release that was previously live, if any |
| `releaseId`                                        | *string*                                           | :heavy_check_mark:                                 | ID of the release that is now live                 |
| `type`                                             | *"DeploymentReleased"*                             | :heavy_check_mark:                                 | N/A                                                |
