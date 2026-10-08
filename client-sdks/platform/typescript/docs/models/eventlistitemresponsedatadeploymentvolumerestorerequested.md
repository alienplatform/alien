# EventListItemResponseDataDeploymentVolumeRestoreRequested

## Example Usage

```typescript
import { EventListItemResponseDataDeploymentVolumeRestoreRequested } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeploymentVolumeRestoreRequested = {
  deploymentId: "<id>",
  ordinal: 139249,
  requestId: "<id>",
  resourceId: "<id>",
  snapshotId: "<id>",
  type: "DeploymentVolumeRestoreRequested",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `actor`                                           | *models.EventListItemResponseActorUnion8*         | :heavy_minus_sign:                                | N/A                                               |
| `deploymentId`                                    | *string*                                          | :heavy_check_mark:                                | ID of the deployment                              |
| `ordinal`                                         | *number*                                          | :heavy_check_mark:                                | Replica ordinal whose volume is replaced          |
| `requestId`                                       | *string*                                          | :heavy_check_mark:                                | ID of the volume restore request                  |
| `resourceId`                                      | *string*                                          | :heavy_check_mark:                                | ID of the container resource that owns the volume |
| `snapshotId`                                      | *string*                                          | :heavy_check_mark:                                | Cloud ID of the snapshot to restore               |
| `type`                                            | *"DeploymentVolumeRestoreRequested"*              | :heavy_check_mark:                                | N/A                                               |
