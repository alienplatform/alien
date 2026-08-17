# APIKeyDeploymentSetupConfig

## Example Usage

```typescript
import { APIKeyDeploymentSetupConfig } from "@alienplatform/platform-api/models";

let value: APIKeyDeploymentSetupConfig = {
  metadata: {
    "key": "<value>",
  },
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [
      "google-oauth",
    ],
  },
  environmentVariables: [
    {
      name: "<value>",
      type: "plain",
      targetResources: [],
    },
  ],
  items: [
    {
      item: "keys",
      source: {
        type: "built-in",
        definitionId: "customer-registry",
        version: "<value>",
        sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      },
      required: false,
    },
  ],
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `metadata`                                                                                                 | Record<string, *any*>                                                                                      | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `policy`                                                                                                   | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                         | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `environmentVariables`                                                                                     | [models.APIKeyDeploymentSetupEnvironmentVariable](../models/apikeydeploymentsetupenvironmentvariable.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `items`                                                                                                    | [models.APIKeyDeploymentSetupConfigItem](../models/apikeydeploymentsetupconfigitem.md)[]                   | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |