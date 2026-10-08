# EventDataDeploymentVolumeRestoreRequested

## Example Usage

```typescript
import { EventDataDeploymentVolumeRestoreRequested } from "@alienplatform/platform-api/models";

let value: EventDataDeploymentVolumeRestoreRequested = {
  deploymentId: "<id>",
  ordinal: 396083,
  requestId: "<id>",
  resourceId: "<id>",
  snapshotId: "<id>",
  type: "DeploymentVolumeRestoreRequested",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `actor`                                           | *models.EventActorUnion8*                         | :heavy_minus_sign:                                | N/A                                               |
| `deploymentId`                                    | *string*                                          | :heavy_check_mark:                                | ID of the deployment                              |
| `ordinal`                                         | *number*                                          | :heavy_check_mark:                                | Replica ordinal whose volume is replaced          |
| `requestId`                                       | *string*                                          | :heavy_check_mark:                                | ID of the volume restore request                  |
| `resourceId`                                      | *string*                                          | :heavy_check_mark:                                | ID of the container resource that owns the volume |
| `snapshotId`                                      | *string*                                          | :heavy_check_mark:                                | Cloud ID of the snapshot to restore               |
| `type`                                            | *"DeploymentVolumeRestoreRequested"*              | :heavy_check_mark:                                | N/A                                               |