# CreateProjectPackagesConfigRequest

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { CreateProjectPackagesConfigRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectPackagesConfigRequest = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                  | [operations.CreateProjectCliRequest](../../models/operations/createprojectclirequest.md)                               | :heavy_minus_sign:                                                                                                     | CLI package configuration. If null, CLI packages will not be generated.                                                |
| `cloudformation`                                                                                                       | [operations.CreateProjectCloudformationRequest](../../models/operations/createprojectcloudformationrequest.md)         | :heavy_minus_sign:                                                                                                     | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                          |
| `agentImage`                                                                                                           | [operations.CreateProjectAgentImageRequest](../../models/operations/createprojectagentimagerequest.md)                 | :heavy_minus_sign:                                                                                                     | Agent image package configuration. Required when Helm is enabled. If null, agent image packages will not be generated. |
| `helm`                                                                                                                 | [operations.CreateProjectHelmRequest](../../models/operations/createprojecthelmrequest.md)                             | :heavy_minus_sign:                                                                                                     | Helm chart package configuration. If null, Helm packages will not be generated.                                        |
| `terraform`                                                                                                            | [operations.CreateProjectTerraformRequest](../../models/operations/createprojectterraformrequest.md)                   | :heavy_minus_sign:                                                                                                     | Terraform provider package configuration. If null, Terraform packages will not be generated.                           |