# DeploymentInfoSetupConfigInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { DeploymentInfoSetupConfigInput } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfigInput = {
  description: "advanced masticate pfft",
  id: "<id>",
  kind: "secret",
  label: "<value>",
  providedBy: [
    "developer",
  ],
  required: false,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `default`                                                                                          | *models.DeploymentInfoSetupConfigDefaultUnion*                                                     | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `description`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | Human-facing helper text.                                                                          |
| `env`                                                                                              | [models.DeploymentInfoSetupConfigEnv](../models/deploymentinfosetupconfigenv.md)[]                 | :heavy_minus_sign:                                                                                 | Runtime env-var mappings for v1 input resolution.                                                  |
| `id`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | Stable input ID used by CLI/API calls.                                                             |
| `kind`                                                                                             | [models.DeploymentInfoSetupConfigKind](../models/deploymentinfosetupconfigkind.md)                 | :heavy_check_mark:                                                                                 | Primitive stack input kind.                                                                        |
| `label`                                                                                            | *string*                                                                                           | :heavy_check_mark:                                                                                 | Human-facing field label.                                                                          |
| `placeholder`                                                                                      | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Example placeholder shown in UI.                                                                   |
| `platforms`                                                                                        | [models.DeploymentInfoSetupConfigPlatform](../models/deploymentinfosetupconfigplatform.md)[]       | :heavy_minus_sign:                                                                                 | Platforms where this input applies.                                                                |
| `providedBy`                                                                                       | [models.DeploymentInfoSetupConfigProvidedBy](../models/deploymentinfosetupconfigprovidedby.md)[]   | :heavy_check_mark:                                                                                 | Who can provide this value.                                                                        |
| `required`                                                                                         | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | Whether a resolved value is required before deployment can proceed.                                |
| `setupMethods`                                                                                     | [models.DeploymentInfoSetupConfigSetupMethod](../models/deploymentinfosetupconfigsetupmethod.md)[] | :heavy_minus_sign:                                                                                 | Setup methods where this input applies.                                                            |
| `validation`                                                                                       | *models.DeploymentInfoSetupConfigValidationUnion*                                                  | :heavy_minus_sign:                                                                                 | N/A                                                                                                |