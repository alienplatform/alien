# DeploymentInstallation

## Example Usage

```typescript
import { DeploymentInstallation } from "@alienplatform/platform-api/models";

let value: DeploymentInstallation = {
  method: "cli",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `method`                                                                               | [models.DeploymentInstallationMethod](../models/deploymentinstallationmethod.md)       | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `preparationId`                                                                        | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `managerUrl`                                                                           | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `region`                                                                               | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `stackName`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `launchUrl`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `terraform`                                                                            | [models.DeploymentInstallationTerraform](../models/deploymentinstallationterraform.md) | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `helm`                                                                                 | [models.DeploymentInstallationHelm](../models/deploymentinstallationhelm.md)           | :heavy_minus_sign:                                                                     | N/A                                                                                    |