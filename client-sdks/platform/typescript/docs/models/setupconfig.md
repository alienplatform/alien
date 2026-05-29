# SetupConfig

## Example Usage

```typescript
import { SetupConfig } from "@alienplatform/platform-api/models";

let value: SetupConfig = {
  metadata: {},
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
  environmentVariables: [
    {
      name: "<value>",
      type: "plain",
      targetResources: [
        "<value 1>",
        "<value 2>",
      ],
    },
  ],
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `metadata`                                                                                                 | Record<string, *any*>                                                                                      | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `policy`                                                                                                   | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                         | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `environmentVariables`                                                                                     | [models.CreateManagerResponseEnvironmentVariable](../models/createmanagerresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |