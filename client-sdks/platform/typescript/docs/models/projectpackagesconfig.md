# ProjectPackagesConfig

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { ProjectPackagesConfig } from "@alienplatform/platform-api/models";

let value: ProjectPackagesConfig = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                  | [models.ProjectCli](../models/projectcli.md)                                                                           | :heavy_minus_sign:                                                                                                     | CLI package configuration. If null, CLI packages will not be generated.                                                |
| `cloudformation`                                                                                                       | [models.ProjectCloudformation](../models/projectcloudformation.md)                                                     | :heavy_minus_sign:                                                                                                     | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                          |
| `agentImage`                                                                                                           | [models.ProjectAgentImage](../models/projectagentimage.md)                                                             | :heavy_minus_sign:                                                                                                     | Agent image package configuration. Required when Helm is enabled. If null, agent image packages will not be generated. |
| `helm`                                                                                                                 | [models.ProjectHelm](../models/projecthelm.md)                                                                         | :heavy_minus_sign:                                                                                                     | Helm chart package configuration. If null, Helm packages will not be generated.                                        |
| `terraform`                                                                                                            | [models.ProjectTerraform](../models/projectterraform.md)                                                               | :heavy_minus_sign:                                                                                                     | Terraform provider package configuration. If null, Terraform packages will not be generated.                           |