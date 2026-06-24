# ManagerRetryResponseSetupConfig

## Example Usage

```typescript
import { ManagerRetryResponseSetupConfig } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseSetupConfig = {
  metadata: {
    "key": "<value>",
    "key1": "<value>",
  },
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [],
  },
  environmentVariables: [],
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `metadata`                                                                                               | Record<string, *any*>                                                                                    | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `policy`                                                                                                 | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `environmentVariables`                                                                                   | [models.ManagerRetryResponseEnvironmentVariable](../models/managerretryresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |