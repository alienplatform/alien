# NewDeploymentRequest

Request schema for creating a new agent

## Example Usage

```typescript
import { NewDeploymentRequest } from "@aliendotdev/platform-api/models";

let value: NewDeploymentRequest = {
  name: "acme-prod",
  platform: "local",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  pinnedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  project: "<value>",
};
```

## Fields

| Field                                                                                             | Type                                                                                              | Required                                                                                          | Description                                                                                       | Example                                                                                           |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `name`                                                                                            | *string*                                                                                          | :heavy_check_mark:                                                                                | Deployment name.                                                                                  | acme-prod                                                                                         |
| `platform`                                                                                        | [models.NewDeploymentRequestPlatform](../models/newdeploymentrequestplatform.md)                  | :heavy_check_mark:                                                                                | Target platform for the deployment                                                                |                                                                                                   |
| `deploymentGroupId`                                                                               | *string*                                                                                          | :heavy_minus_sign:                                                                                | Required for workspace/project tokens. Deployment group tokens use their own group automatically. |                                                                                                   |
| `managerId`                                                                                       | *string*                                                                                          | :heavy_minus_sign:                                                                                | ID of the manager responsible for this deployment                                                 | mgr_enxscjrqiiu2lrc672hwwuc5                                                                      |
| `pinnedReleaseId`                                                                                 | *string*                                                                                          | :heavy_minus_sign:                                                                                | ID of the pinned release                                                                          | rel_WbhQgksrawSKIpEN0NAssHX9                                                                      |
| `environmentVariables`                                                                            | [models.EnvironmentVariableConfig](../models/environmentvariableconfig.md)[]                      | :heavy_minus_sign:                                                                                | Configuration of environment variables for the deployment                                         |                                                                                                   |
| `environmentInfo`                                                                                 | *models.NewDeploymentRequestEnvironmentInfoUnion*                                                 | :heavy_minus_sign:                                                                                | Cloud environment information                                                                     |                                                                                                   |
| `project`                                                                                         | *string*                                                                                          | :heavy_check_mark:                                                                                | Project ID or name                                                                                |                                                                                                   |
| `stackSettings`                                                                                   | [models.NewDeploymentRequestStackSettings](../models/newdeploymentrequeststacksettings.md)        | :heavy_minus_sign:                                                                                | Stack settings for deployment customization                                                       |                                                                                                   |