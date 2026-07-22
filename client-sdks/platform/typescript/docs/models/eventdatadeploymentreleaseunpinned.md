# EventDataDeploymentReleaseUnpinned

## Example Usage

```typescript
import { EventDataDeploymentReleaseUnpinned } from "@alienplatform/platform-api/models";

let value: EventDataDeploymentReleaseUnpinned = {
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
