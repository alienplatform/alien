# EventListItemResponseDataDeploymentReleaseUnpinned

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentReleaseUnpinned } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentReleaseUnpinned = {
  deploymentId: "<id>",
  previousPinnedReleaseId: "<id>",
  type: "DeploymentReleaseUnpinned",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `deploymentId`                               | *string*                                     | :heavy_check_mark:                           | ID of the deployment                         |
| `previousPinnedReleaseId`                    | *string*                                     | :heavy_check_mark:                           | ID of the release that was previously pinned |
| `type`                                       | *"DeploymentReleaseUnpinned"*                | :heavy_check_mark:                           | N/A                                          |
