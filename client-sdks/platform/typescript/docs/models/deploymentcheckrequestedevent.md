# DeploymentCheckRequestedEvent

## Example Usage

```typescript
import { DeploymentCheckRequestedEvent } from "@alienplatform/platform-api/models";

let value: DeploymentCheckRequestedEvent = {
  type: "DeploymentCheckRequested",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  | Example                                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `type`                                                                                       | *"DeploymentCheckRequested"*                                                                 | :heavy_check_mark:                                                                           | N/A                                                                                          |                                                                                              |
| `deploymentId`                                                                               | *string*                                                                                     | :heavy_check_mark:                                                                           | Unique identifier for the deployment.                                                        | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                             |
| `actor`                                                                                      | [models.DeploymentCheckRequestedEventActor](../models/deploymentcheckrequestedeventactor.md) | :heavy_minus_sign:                                                                           | Authenticated principal that requested a deployment intent event.                            |                                                                                              |