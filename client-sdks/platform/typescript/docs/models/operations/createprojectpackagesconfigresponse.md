# CreateProjectPackagesConfigResponse

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { CreateProjectPackagesConfigResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectPackagesConfigResponse = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                        | [operations.CreateProjectCliResponse](../../models/operations/createprojectcliresponse.md)                                   | :heavy_minus_sign:                                                                                                           | CLI package configuration. If null, CLI packages will not be generated.                                                      |
| `cloudformation`                                                                                                             | [operations.CreateProjectCloudformationResponse](../../models/operations/createprojectcloudformationresponse.md)             | :heavy_minus_sign:                                                                                                           | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                                |
| `operatorImage`                                                                                                              | [operations.CreateProjectOperatorImageResponse](../../models/operations/createprojectoperatorimageresponse.md)               | :heavy_minus_sign:                                                                                                           | Operator image package configuration. Required when Helm is enabled. If null, operator image packages will not be generated. |
| `helm`                                                                                                                       | [operations.CreateProjectHelmResponse](../../models/operations/createprojecthelmresponse.md)                                 | :heavy_minus_sign:                                                                                                           | Helm chart package configuration. If null, Helm packages will not be generated.                                              |
| `terraform`                                                                                                                  | [operations.CreateProjectTerraformResponse](../../models/operations/createprojectterraformresponse.md)                       | :heavy_minus_sign:                                                                                                           | Terraform provider package configuration. If null, Terraform packages will not be generated.                                 |