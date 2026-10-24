# DeploymentInstallation

## Example Usage

```typescript
import { DeploymentInstallation } from "@alienplatform/platform-api/models";

let value: DeploymentInstallation = {
  method: "terraform",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `method`                                                                               | [models.DeploymentInstallationMethod](../models/deploymentinstallationmethod.md)       | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `region`                                                                               | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `stackName`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `launchUrl`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `terraform`                                                                            | [models.DeploymentInstallationTerraform](../models/deploymentinstallationterraform.md) | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `helm`                                                                                 | [models.DeploymentInstallationHelm](../models/deploymentinstallationhelm.md)           | :heavy_minus_sign:                                                                     | N/A                                                                                    |