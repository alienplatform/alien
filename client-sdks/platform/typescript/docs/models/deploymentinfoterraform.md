# DeploymentInfoTerraform

## Example Usage

```typescript
import { DeploymentInfoTerraform } from "@alienplatform/platform-api/models";

let value: DeploymentInfoTerraform = {
  status: "canceled",
  providerSource: "<value>",
  moduleSources: {
    "key": "<value>",
  },
  managerUrls: {
    "key": "<value>",
    "key1": "<value>",
  },
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `status`                                                 | [models.TerraformStatus](../models/terraformstatus.md)   | :heavy_check_mark:                                       | Status of a package build                                |
| `version`                                                | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `outputs`                                                | [models.TerraformOutputs](../models/terraformoutputs.md) | :heavy_minus_sign:                                       | Outputs from a Terraform package build.                  |
| `error`                                                  | *any*                                                    | :heavy_minus_sign:                                       | N/A                                                      |
| `providerSource`                                         | *string*                                                 | :heavy_check_mark:                                       | Terraform provider source (without https://)             |
| `moduleSources`                                          | Record<string, *string*>                                 | :heavy_check_mark:                                       | Terraform module sources by target                       |
| `moduleVersion`                                          | *string*                                                 | :heavy_minus_sign:                                       | N/A                                                      |
| `managerUrls`                                            | Record<string, *string*>                                 | :heavy_check_mark:                                       | Manager URLs by Terraform target                         |