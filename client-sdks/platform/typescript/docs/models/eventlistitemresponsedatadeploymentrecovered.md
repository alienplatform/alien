# EventListItemResponseDataDeploymentRecovered

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentRecovered } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentRecovered = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRecovered",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `deploymentId`                     | *string*                           | :heavy_check_mark:                 | ID of the deployment               |
| `releaseId`                        | *string*                           | :heavy_check_mark:                 | ID of the release that is now live |
| `type`                             | *"DeploymentRecovered"*            | :heavy_check_mark:                 | N/A                                |
