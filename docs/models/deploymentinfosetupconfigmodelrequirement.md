# DeploymentInfoSetupConfigModelRequirement

## Example Usage

```typescript
import { DeploymentInfoSetupConfigModelRequirement } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfigModelRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "openai-chat",
  ],
  required: true,
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                                | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `clientApis`                                                                                   | [models.DeploymentInfoSetupConfigClientApi](../models/deploymentinfosetupconfigclientapi.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `required`                                                                                     | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |