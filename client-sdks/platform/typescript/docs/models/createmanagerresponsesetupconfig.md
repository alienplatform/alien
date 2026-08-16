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
    allowedSetupMethods: [],
  },
  items: [
    {
      item: "alien-stack",
      source: {
        type: "application-release",
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

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `metadata`                                                                                                 | Record<string, *any*>                                                                                      | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `policy`                                                                                                   | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                         | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `inputValues`                                                                                              | Record<string, [models.EncryptedStackInputValue](../models/encryptedstackinputvalue.md)>                   | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `items`                                                                                                    | [models.CreateManagerResponseItem](../models/createmanagerresponseitem.md)[]                               | :heavy_minus_sign:                                                                                         | Immutable setup items and exact sources captured when this setup link is created.                          |
| `publicSubdomain`                                                                                          | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Operator-pinned deployment subdomain for this setup token.                                                 |
| `environmentVariables`                                                                                     | [models.CreateManagerResponseEnvironmentVariable](../models/createmanagerresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |