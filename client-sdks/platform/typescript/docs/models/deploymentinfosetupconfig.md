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
    allowedSetupMethods: [],
  },
  environmentVariables: [],
  items: [
    {
      item: "alien-stack",
      source: {
        type: "built-in",
        definitionId: "customer-ai",
        version: "<value>",
        sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      },
      required: false,
    },
  ],
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `metadata`                                                                                                         | Record<string, *any*>                                                                                              | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `policy`                                                                                                           | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                                 | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `environmentVariables`                                                                                             | [models.DeploymentInfoSetupConfigEnvironmentVariable](../models/deploymentinfosetupconfigenvironmentvariable.md)[] | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `items`                                                                                                            | [models.DeploymentInfoSetupConfigItem](../models/deploymentinfosetupconfigitem.md)[]                               | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `inputs`                                                                                                           | [models.DeploymentInfoSetupConfigInput](../models/deploymentinfosetupconfiginput.md)[]                             | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `inputValues`                                                                                                      | [models.ResolvedStackInputSummary](../models/resolvedstackinputsummary.md)[]                                       | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |