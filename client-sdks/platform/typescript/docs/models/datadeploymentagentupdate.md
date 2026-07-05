# DataDeploymentAgentUpdate

## Example Usage

```typescript
import { DataDeploymentAgentUpdate } from "@alienplatform/platform-api/models";

let value: DataDeploymentAgentUpdate = {
  deploymentId: "<id>",
  toAgentVersion: "<value>",
  type: "DeploymentAgentUpdate",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `deploymentId`                                                             | *string*                                                                   | :heavy_check_mark:                                                         | ID of the deployment being updated                                         |
| `fromAgentVersion`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | Agent version the deployment was running when the update started, if known |
| `toAgentVersion`                                                           | *string*                                                                   | :heavy_check_mark:                                                         | Target agent version the deployment was pinned to                          |
| `type`                                                                     | *"DeploymentAgentUpdate"*                                                  | :heavy_check_mark:                                                         | N/A                                                                        |