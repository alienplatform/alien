# CreateDeploymentGroupTokenRequest

## Example Usage

```typescript
import { CreateDeploymentGroupTokenRequest } from "@alienplatform/platform-api/models";

let value: CreateDeploymentGroupTokenRequest = {
  deploymentSetupConfig: {
    metadata: {},
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [],
    },
    environmentVariables: [],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `description`                                                                                 | *string*                                                                                      | :heavy_minus_sign:                                                                            | Description for the API key                                                                   |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Optional expiration date for the API key                                                      |
| `deploymentSetupConfig`                                                                       | [models.DeploymentSetupConfig](../models/deploymentsetupconfig.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `setupItems`                                                                                  | [models.DeploymentSetupItemSelection](../models/deploymentsetupitemselection.md)[]            | :heavy_minus_sign:                                                                            | Customer infrastructure to include. The server snapshots its exact reviewed sources.          |
| `inputValues`                                                                                 | Record<string, *models.StackInputValueRequest*>                                               | :heavy_minus_sign:                                                                            | N/A                                                                                           |