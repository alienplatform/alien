# DeploymentSetupConfig

## Example Usage

```typescript
import { DeploymentSetupConfig } from "@alienplatform/platform-api/models";

let value: DeploymentSetupConfig = {
  metadata: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [],
  },
  environmentVariables: [],
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `metadata`                                                                   | Record<string, *any*>                                                        | :heavy_check_mark:                                                           | N/A                                                                          |
| `policy`                                                                     | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)           | :heavy_check_mark:                                                           | N/A                                                                          |
| `environmentVariables`                                                       | [models.EnvironmentVariableConfig](../models/environmentvariableconfig.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |