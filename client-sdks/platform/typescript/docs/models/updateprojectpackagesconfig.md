# UpdateProjectPackagesConfig

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { UpdateProjectPackagesConfig } from "@alienplatform/platform-api/models";

let value: UpdateProjectPackagesConfig = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                        | [models.UpdateProjectPackagesConfigCli](../models/updateprojectpackagesconfigcli.md)                                         | :heavy_minus_sign:                                                                                                           | CLI package configuration. If null, CLI packages will not be generated.                                                      |
| `cloudformation`                                                                                                             | [models.UpdateProjectPackagesConfigCloudformation](../models/updateprojectpackagesconfigcloudformation.md)                   | :heavy_minus_sign:                                                                                                           | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                                |
| `operatorImage`                                                                                                              | [models.UpdateProjectPackagesConfigOperatorImage](../models/updateprojectpackagesconfigoperatorimage.md)                     | :heavy_minus_sign:                                                                                                           | Operator image package configuration. Required when Helm is enabled. If null, Operator image packages will not be generated. |
| `helm`                                                                                                                       | [models.UpdateProjectPackagesConfigHelm](../models/updateprojectpackagesconfighelm.md)                                       | :heavy_minus_sign:                                                                                                           | Helm chart package configuration. If null, Helm packages will not be generated.                                              |
| `terraform`                                                                                                                  | [models.UpdateProjectPackagesConfigTerraform](../models/updateprojectpackagesconfigterraform.md)                             | :heavy_minus_sign:                                                                                                           | Terraform package configuration. If null, Terraform packages will not be generated.                                          |