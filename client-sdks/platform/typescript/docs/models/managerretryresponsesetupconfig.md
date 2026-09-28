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
  validatedReleaseSelection: {
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    releaseChannel: "<value>",
    platform: "aws",
  },
  items: [
    {
      item: "models",
      source: {
        type: "built-in",
        definitionId: "customer-ai",
        version: "<value>",
        sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      },
      required: true,
    },
  ],
  environmentVariables: [],
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `metadata`                                                                                                         | Record<string, *any*>                                                                                              | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `policy`                                                                                                           | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                                 | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `inputValues`                                                                                                      | Record<string, [models.EncryptedStackInputValue](../models/encryptedstackinputvalue.md)>                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `validatedReleaseSelection`                                                                                        | [models.ManagerRetryResponseValidatedReleaseSelection](../models/managerretryresponsevalidatedreleaseselection.md) | :heavy_minus_sign:                                                                                                 | Immutable Release whose schema validated first-party inputs for this platform and channel.                         |
| `items`                                                                                                            | [models.ManagerRetryResponseItem](../models/managerretryresponseitem.md)[]                                         | :heavy_minus_sign:                                                                                                 | Immutable setup items and exact sources captured when this setup link is created.                                  |
| `publicSubdomain`                                                                                                  | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Operator-pinned deployment subdomain for this setup token.                                                         |
| `environmentVariables`                                                                                             | [models.ManagerRetryResponseEnvironmentVariable](../models/managerretryresponseenvironmentvariable.md)[]           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |