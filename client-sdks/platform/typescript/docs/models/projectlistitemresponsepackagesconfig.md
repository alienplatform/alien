# ProjectListItemResponsePackagesConfig

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { ProjectListItemResponsePackagesConfig } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponsePackagesConfig = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                        | [models.ProjectListItemResponseCli](../models/projectlistitemresponsecli.md)                                                 | :heavy_minus_sign:                                                                                                           | CLI package configuration. If null, CLI packages will not be generated.                                                      |
| `cloudformation`                                                                                                             | [models.ProjectListItemResponseCloudformation](../models/projectlistitemresponsecloudformation.md)                           | :heavy_minus_sign:                                                                                                           | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                                |
| `operatorImage`                                                                                                              | [models.ProjectListItemResponseOperatorImage](../models/projectlistitemresponseoperatorimage.md)                             | :heavy_minus_sign:                                                                                                           | Operator image package configuration. Required when Helm is enabled. If null, operator image packages will not be generated. |
| `helm`                                                                                                                       | [models.ProjectListItemResponseHelm](../models/projectlistitemresponsehelm.md)                                               | :heavy_minus_sign:                                                                                                           | Helm chart package configuration. If null, Helm packages will not be generated.                                              |
| `terraform`                                                                                                                  | [models.ProjectListItemResponseTerraform](../models/projectlistitemresponseterraform.md)                                     | :heavy_minus_sign:                                                                                                           | Terraform provider package configuration. If null, Terraform packages will not be generated.                                 |