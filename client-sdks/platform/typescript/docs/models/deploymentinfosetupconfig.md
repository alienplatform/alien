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
    release: {
      mode: "fixed",
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    },
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