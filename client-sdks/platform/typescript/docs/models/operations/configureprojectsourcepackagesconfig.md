# ConfigureProjectSourcePackagesConfig

Configuration for embedded packages (CLI, CloudFormation, Helm, Terraform)

## Example Usage

```typescript
import { ConfigureProjectSourcePackagesConfig } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourcePackagesConfig = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `cli`                                                                                                                        | [operations.ConfigureProjectSourceCli](../../models/operations/configureprojectsourcecli.md)                                 | :heavy_minus_sign:                                                                                                           | CLI package configuration. If null, CLI packages will not be generated.                                                      |
| `cloudformation`                                                                                                             | [operations.ConfigureProjectSourceCloudformation](../../models/operations/configureprojectsourcecloudformation.md)           | :heavy_minus_sign:                                                                                                           | CloudFormation package configuration. If null, CloudFormation packages will not be generated.                                |
| `operatorImage`                                                                                                              | [operations.ConfigureProjectSourceOperatorImage](../../models/operations/configureprojectsourceoperatorimage.md)             | :heavy_minus_sign:                                                                                                           | Operator image package configuration. Required when Helm is enabled. If null, Operator image packages will not be generated. |
| `helm`                                                                                                                       | [operations.ConfigureProjectSourceHelm](../../models/operations/configureprojectsourcehelm.md)                               | :heavy_minus_sign:                                                                                                           | Helm chart package configuration. If null, Helm packages will not be generated.                                              |
| `terraform`                                                                                                                  | [operations.ConfigureProjectSourceTerraform](../../models/operations/configureprojectsourceterraform.md)                     | :heavy_minus_sign:                                                                                                           | Terraform package configuration. If null, Terraform packages will not be generated.                                          |