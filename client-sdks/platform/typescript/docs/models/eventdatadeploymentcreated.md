# EventDataDeploymentCreated

## Example Usage

```typescript
import { EventDataDeploymentCreated } from "@alienplatform/platform-api/models";

let value: EventDataDeploymentCreated = {
  deploymentGroupId: "<id>",
  deploymentId: "<id>",
  type: "DeploymentCreated",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `deploymentGroupId`                               | *string*                                          | :heavy_check_mark:                                | ID of the deployment group this slot belongs to   |
| `deploymentId`                                    | *string*                                          | :heavy_check_mark:                                | ID of the deployment that was created             |
| `releaseId`                                       | *string*                                          | :heavy_minus_sign:                                | Initial release the slot was created with, if any |
| `type`                                            | *"DeploymentCreated"*                             | :heavy_check_mark:                                | N/A                                               |
