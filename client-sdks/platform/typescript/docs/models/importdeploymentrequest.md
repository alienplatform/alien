# ImportDeploymentRequest

Request schema for importing an agent from existing infrastructure

## Example Usage

```typescript
import { ImportDeploymentRequest } from "@aliendotdev/platform-api/models";

let value: ImportDeploymentRequest = {
  name: "acme-prod",
  platform: "kubernetes",
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  project: "<value>",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  source: {
    type: "cloudformation",
    stackName: "<value>",
    region: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            | Example                                                                                |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `name`                                                                                 | *string*                                                                               | :heavy_check_mark:                                                                     | Deployment name.                                                                       | acme-prod                                                                              |
| `platform`                                                                             | [models.ImportDeploymentRequestPlatform](../models/importdeploymentrequestplatform.md) | :heavy_check_mark:                                                                     | Target platform for the deployment                                                     |                                                                                        |
| `deploymentGroupId`                                                                    | *string*                                                                               | :heavy_check_mark:                                                                     | ID of deployment group this deployment belongs to                                      | dg_r27ict8c7vcgsumpj90ackf7b                                                           |
| `project`                                                                              | *string*                                                                               | :heavy_check_mark:                                                                     | Project ID or name                                                                     |                                                                                        |
| `managerId`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | Agent manager ID (required for import operations)                                      | mgr_enxscjrqiiu2lrc672hwwuc5                                                           |
| `source`                                                                               | [models.Source](../models/source.md)                                                   | :heavy_check_mark:                                                                     | Import source configuration                                                            |                                                                                        |