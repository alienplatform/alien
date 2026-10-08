# EventDataDeploymentVolumeRestoreCancelled

## Example Usage

```typescript
import { EventDataDeploymentVolumeRestoreCancelled } from "@alienplatform/platform-api/models";

let value: EventDataDeploymentVolumeRestoreCancelled = {
  deploymentId: "<id>",
  ordinal: 25942,
  requestId: "<id>",
  resourceId: "<id>",
  type: "DeploymentVolumeRestoreCancelled",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `actor`                                           | *models.EventActorUnion9*                         | :heavy_minus_sign:                                | N/A                                               |
| `deploymentId`                                    | *string*                                          | :heavy_check_mark:                                | ID of the deployment                              |
| `ordinal`                                         | *number*                                          | :heavy_check_mark:                                | Replica ordinal whose volume was to be replaced   |
| `requestId`                                       | *string*                                          | :heavy_check_mark:                                | ID of the cancelled volume restore request        |
| `resourceId`                                      | *string*                                          | :heavy_check_mark:                                | ID of the container resource that owns the volume |
| `type`                                            | *"DeploymentVolumeRestoreCancelled"*              | :heavy_check_mark:                                | N/A                                               |
