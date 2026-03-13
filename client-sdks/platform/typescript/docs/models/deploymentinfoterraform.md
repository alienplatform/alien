# DeploymentInfoTerraform

## Example Usage

```typescript
import { DeploymentInfoTerraform } from "@aliendotdev/platform-api/models";

let value: DeploymentInfoTerraform = {
  status: "canceled",
  providerSource: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `status`                                                 | [models.TerraformStatus](../models/terraformstatus.md)   | :heavy_check_mark:                                       | Status of a package build                                |
| `version`                                                | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `outputs`                                                | [models.TerraformOutputs](../models/terraformoutputs.md) | :heavy_minus_sign:                                       | Outputs from a Terraform provider package build          |
| `error`                                                  | *any*                                                    | :heavy_minus_sign:                                       | N/A                                                      |
| `providerSource`                                         | *string*                                                 | :heavy_check_mark:                                       | Terraform provider source (without https://)             |