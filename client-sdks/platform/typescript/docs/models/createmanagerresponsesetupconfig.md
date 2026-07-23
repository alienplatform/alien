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
| `inputValues`                                                                                              | Record<string, [models.EncryptedStackInputValue](../models/encryptedstackinputvalue.md)>                   | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `publicSubdomain`                                                                                          | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Operator-pinned deployment subdomain for this setup token.                                                 |
| `environmentVariables`                                                                                     | [models.CreateManagerResponseEnvironmentVariable](../models/createmanagerresponseenvironmentvariable.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
