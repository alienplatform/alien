# EventListItemResponseDataDeploymentReleasePinned

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentReleasePinned } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentReleasePinned = {
  deploymentId: "<id>",
  pinnedReleaseId: "<id>",
  type: "DeploymentReleasePinned",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `deploymentId`                              | *string*                                    | :heavy_check_mark:                          | ID of the deployment                        |
| `pinnedReleaseId`                           | *string*                                    | :heavy_check_mark:                          | ID of the release that is now pinned        |
| `previousPinnedReleaseId`                   | *string*                                    | :heavy_minus_sign:                          | ID of the previously pinned release, if any |
| `type`                                      | *"DeploymentReleasePinned"*                 | :heavy_check_mark:                          | N/A                                         |
