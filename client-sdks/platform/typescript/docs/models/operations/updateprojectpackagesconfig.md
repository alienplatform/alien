# UpdateProjectPackagesConfig

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { UpdateProjectPackagesConfig } from "@aliendotdev/platform-api/models/operations";

let value: UpdateProjectPackagesConfig = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                        | [operations.UpdateProjectCli](../../models/operations/updateprojectcli.md)                                                   | :heavy_minus_sign:                                                                                                           | CLI package configuration. If null, CLI packages will not be generated.                                                      |
| `cloudformation`                                                                                                             | [operations.UpdateProjectCloudformation](../../models/operations/updateprojectcloudformation.md)                             | :heavy_minus_sign:                                                                                                           | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                                |
| `operatorImage`                                                                                                              | [operations.UpdateProjectOperatorImage](../../models/operations/updateprojectoperatorimage.md)                               | :heavy_minus_sign:                                                                                                           | Operator image package configuration. Required when Helm is enabled. If null, operator image packages will not be generated. |
| `helm`                                                                                                                       | [operations.UpdateProjectHelm](../../models/operations/updateprojecthelm.md)                                                 | :heavy_minus_sign:                                                                                                           | Helm chart package configuration. If null, Helm packages will not be generated.                                              |
| `terraform`                                                                                                                  | [operations.UpdateProjectTerraform](../../models/operations/updateprojectterraform.md)                                       | :heavy_minus_sign:                                                                                                           | Terraform provider package configuration. If null, Terraform packages will not be generated.                                 |