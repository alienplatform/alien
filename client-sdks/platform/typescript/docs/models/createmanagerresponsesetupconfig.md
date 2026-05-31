# CreateManagerResponseSetupConfig

## Example Usage

```typescript
import { CreateManagerResponseSetupConfig } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseSetupConfig = {
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

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `metadata`                                                                                                 | Record<string, *any*>                                                                                      | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `policy`                                                                                                   | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                         | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `environmentVariables`                                                                                     | [models.CreateManagerResponseEnvironmentVariable](../models/createmanagerresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |