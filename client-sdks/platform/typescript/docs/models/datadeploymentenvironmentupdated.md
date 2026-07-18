# DataDeploymentEnvironmentUpdated

## Example Usage

```typescript
import { DataDeploymentEnvironmentUpdated } from "@alienplatform/platform-api/models";

let value: DataDeploymentEnvironmentUpdated = {
  changedKeys: [],
  deploymentId: "<id>",
  type: "DeploymentEnvironmentUpdated",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `actor`                                                                       | *models.ActorUnion2*                                                          | :heavy_minus_sign:                                                            | N/A                                                                           |
| `changedKeys`                                                                 | *string*[]                                                                    | :heavy_check_mark:                                                            | Names of the environment variables that changed (added, removed, or modified) |
| `deploymentId`                                                                | *string*                                                                      | :heavy_check_mark:                                                            | ID of the deployment                                                          |
| `type`                                                                        | *"DeploymentEnvironmentUpdated"*                                              | :heavy_check_mark:                                                            | N/A                                                                           |