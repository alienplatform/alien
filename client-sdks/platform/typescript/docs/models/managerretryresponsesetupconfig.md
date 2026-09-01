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
  items: [
    {
      item: "deployment",
      source: {
        type: "project-release",
        releaseChannel: "<value>",
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      },
      required: false,
    },
  ],
  environmentVariables: [],
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `metadata`                                                                                               | Record<string, *any*>                                                                                    | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `policy`                                                                                                 | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `inputValues`                                                                                            | Record<string, [models.EncryptedStackInputValue](../models/encryptedstackinputvalue.md)>                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `items`                                                                                                  | [models.ManagerRetryResponseItem](../models/managerretryresponseitem.md)[]                               | :heavy_minus_sign:                                                                                       | Immutable setup items and exact sources captured when this setup link is created.                        |
| `publicSubdomain`                                                                                        | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | Operator-pinned deployment subdomain for this setup token.                                               |
| `environmentVariables`                                                                                   | [models.ManagerRetryResponseEnvironmentVariable](../models/managerretryresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |