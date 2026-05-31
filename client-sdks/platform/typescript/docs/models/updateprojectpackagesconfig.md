# UpdateProjectPackagesConfig

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { UpdateProjectPackagesConfig } from "@alienplatform/platform-api/models";

let value: UpdateProjectPackagesConfig = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                  | [models.UpdateProjectCli](../models/updateprojectcli.md)                                                               | :heavy_minus_sign:                                                                                                     | CLI package configuration. If null, CLI packages will not be generated.                                                |
| `cloudformation`                                                                                                       | [models.UpdateProjectCloudformation](../models/updateprojectcloudformation.md)                                         | :heavy_minus_sign:                                                                                                     | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                          |
| `agentImage`                                                                                                           | [models.UpdateProjectAgentImage](../models/updateprojectagentimage.md)                                                 | :heavy_minus_sign:                                                                                                     | Agent image package configuration. Required when Helm is enabled. If null, agent image packages will not be generated. |
| `helm`                                                                                                                 | [models.UpdateProjectHelm](../models/updateprojecthelm.md)                                                             | :heavy_minus_sign:                                                                                                     | Helm chart package configuration. If null, Helm packages will not be generated.                                        |
| `terraform`                                                                                                            | [models.UpdateProjectTerraform](../models/updateprojectterraform.md)                                                   | :heavy_minus_sign:                                                                                                     | Terraform package configuration. If null, Terraform packages will not be generated.                                    |