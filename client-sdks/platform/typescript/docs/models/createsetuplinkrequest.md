# CreateSetupLinkRequest

## Example Usage

```typescript
import { CreateSetupLinkRequest } from "@alienplatform/platform-api/models";

let value: CreateSetupLinkRequest = {
  externalId: "ext_example_01",
  name: "prod-us-east-1",
  project: "<value>",
  deploymentSetupConfig: {
    metadata: {},
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [
        "google-oauth",
      ],
    },
    environmentVariables: [
      {
        name: "<value>",
        value: "<value>",
        type: "plain",
        targetResources: [
          "<value 1>",
          "<value 2>",
        ],
      },
    ],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `externalId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | Case-sensitive, URL- and header-safe identifier from the integrating application.             | ext_example_01                                                                                |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Deployment group name.                                                                        | prod-us-east-1                                                                                |
| `project`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | Project ID or name this deployment group belongs to                                           |                                                                                               |
| `maxDeployments`                                                                              | *number*                                                                                      | :heavy_minus_sign:                                                                            | Maximum number of deployments for newly created groups                                        |                                                                                               |
| `description`                                                                                 | *string*                                                                                      | :heavy_minus_sign:                                                                            | Description for the API key                                                                   |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Optional expiration date for the API key                                                      |                                                                                               |
| `deploymentSetupConfig`                                                                       | [models.DeploymentSetupConfig](../models/deploymentsetupconfig.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `setupItems`                                                                                  | [models.DeploymentSetupItemSelection](../models/deploymentsetupitemselection.md)[]            | :heavy_minus_sign:                                                                            | Customer infrastructure to include. The server snapshots its exact reviewed sources.          |                                                                                               |
| `inputValues`                                                                                 | Record<string, *models.StackInputValueRequest*>                                               | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |