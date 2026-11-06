# KeyInfo

## Example Usage

```typescript
import { KeyInfo } from "@alienplatform/platform-api/models";

let value: KeyInfo = {
  id: "apikey_ye96yxs1tjnrrwulp8frh6w5t6e0",
  description: "carpool ha fiercely brr decongestant ew",
  keyPrefix: "<value>",
  type: "manager",
  role: "<value>",
  additionalRoles: [],
  workspaceId: "<id>",
  projectId: null,
  deploymentId: "<id>",
  deploymentGroupId: null,
  managerId: "<id>",
  enabled: false,
  createdAt: new Date("2026-03-02T19:02:43.885Z"),
  expiresAt: new Date("2026-03-08T23:18:28.226Z"),
  lastUsedAt: new Date("2025-11-09T01:34:30.956Z"),
  revokedAt: new Date("2026-03-31T23:02:08.394Z"),
  deploymentSetupConfig: {
    metadata: {
      "key": "<value>",
      "key1": "<value>",
    },
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [],
    },
    environmentVariables: [
      {
        name: "<value>",
        type: "secret",
        targetResources: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
      },
    ],
    items: [
      {
        item: "models",
        source: {
          type: "built-in",
          definitionId: "customer-sandbox",
          version: "<value>",
          sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        },
        required: true,
      },
    ],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the api key.                                                            | apikey_ye96yxs1tjnrrwulp8frh6w5t6e0                                                           |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `keyPrefix`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `type`                                                                                        | [models.CreateAPIKeyResponseType](../models/createapikeyresponsetype.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `role`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `additionalRoles`                                                                             | [models.ApplicationRole](../models/applicationrole.md)[]                                      | :heavy_check_mark:                                                                            | Application roles a project key holds beside `role`                                           |                                                                                               |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `managerId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `enabled`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `lastUsedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `revokedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentSetupConfig`                                                                       | [models.APIKeyDeploymentSetupConfig](../models/apikeydeploymentsetupconfig.md)                | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |