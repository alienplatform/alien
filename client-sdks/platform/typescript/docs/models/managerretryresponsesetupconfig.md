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
| `inputValues`                                                                                            | Record<string, [models.EncryptedStackInputValue](../models/encryptedstackinputvalue.md)>                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `publicSubdomain`                                                                                        | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | Operator-pinned deployment subdomain for this setup token.                                               |
| `environmentVariables`                                                                                   | [models.ManagerRetryResponseEnvironmentVariable](../models/managerretryresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |