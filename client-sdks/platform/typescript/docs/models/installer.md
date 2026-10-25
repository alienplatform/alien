# Installer

## Example Usage

```typescript
import { Installer } from "@alienplatform/platform-api/models";

let value: Installer = {
  name: "<value>",
  platform: "aws",
  method: "helm",
  configuration: "<value>",
  command: "<value>",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                           | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `platform`                                                                                                       | [models.DeploymentPreparationResponsePlatform](../models/deploymentpreparationresponseplatform.md)               | :heavy_check_mark:                                                                                               | Represents the target cloud platform.                                                                            |
| `method`                                                                                                         | [models.DeploymentPreparationResponseMethod](../models/deploymentpreparationresponsemethod.md)                   | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `configuration`                                                                                                  | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `command`                                                                                                        | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `installCommand`                                                                                                 | *string*                                                                                                         | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `commandVariants`                                                                                                | [models.DeploymentPreparationResponseCommandVariants](../models/deploymentpreparationresponsecommandvariants.md) | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `managerUrl`                                                                                                     | *string*                                                                                                         | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `setupItem`                                                                                                      | [models.DeploymentPreparationResponseSetupItem](../models/deploymentpreparationresponsesetupitem.md)             | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |