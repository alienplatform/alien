# DeploymentGroupMovedEvent

## Example Usage

```typescript
import { DeploymentGroupMovedEvent } from "@alienplatform/platform-api/models";

let value: DeploymentGroupMovedEvent = {
  type: "DeploymentGroupMoved",
  actor: {
    kind: "serviceAccount",
    id: "<id>",
  },
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  previousDeploymentGroupId: "<id>",
  deploymentGroupId: "<id>",
  membershipRevision: 67016,
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          | Example                                                                              |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `type`                                                                               | *"DeploymentGroupMoved"*                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `actor`                                                                              | [models.DeploymentGroupMovedEventActor](../models/deploymentgroupmovedeventactor.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `deploymentId`                                                                       | *string*                                                                             | :heavy_check_mark:                                                                   | Unique identifier for the deployment.                                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                                         |
| `previousDeploymentGroupId`                                                          | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `deploymentGroupId`                                                                  | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
| `membershipRevision`                                                                 | *number*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |                                                                                      |
