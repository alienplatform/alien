# DeploymentInfoSetupConfig

## Example Usage

```typescript
import { DeploymentInfoSetupConfig } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfig = {
  metadata: {
    "key": "<value>",
    "key1": "<value>",
  },
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [
      "google-oauth",
    ],
  },
  environmentVariables: [],
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `metadata`                                                                                                         | Record<string, *any*>                                                                                              | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `policy`                                                                                                           | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                                 | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `environmentVariables`                                                                                             | [models.DeploymentInfoSetupConfigEnvironmentVariable](../models/deploymentinfosetupconfigenvironmentvariable.md)[] | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `inputs`                                                                                                           | [models.DeploymentInfoSetupConfigInput](../models/deploymentinfosetupconfiginput.md)[]                             | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `inputValues`                                                                                                      | [models.ResolvedStackInputSummary](../models/resolvedstackinputsummary.md)[]                                       | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
