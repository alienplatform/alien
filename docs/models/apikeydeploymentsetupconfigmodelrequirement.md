# APIKeyDeploymentSetupConfigModelRequirement

## Example Usage

```typescript
import { APIKeyDeploymentSetupConfigModelRequirement } from "@alienplatform/platform-api/models";

let value: APIKeyDeploymentSetupConfigModelRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "openai-responses",
  ],
  required: false,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `clientApis`                                                                                       | [models.APIKeyDeploymentSetupConfigClientAPI](../models/apikeydeploymentsetupconfigclientapi.md)[] | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `required`                                                                                         | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |