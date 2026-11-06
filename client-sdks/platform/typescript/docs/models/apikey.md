# APIKey

API key information

## Example Usage

```typescript
import { APIKey } from "@alienplatform/platform-api/models";

let value: APIKey = {
  id: "apikey_ye96yxs1tjnrrwulp8frh6w5t6e0",
  description: "hoarse forsaken slowly behind anaesthetise",
  keyPrefix: "<value>",
  type: "manager",
  role: "<value>",
  additionalRoles: [
    "project.ai-gateway",
  ],
  workspaceId: "<id>",
  projectId: "<id>",
  deploymentId: "<id>",
  deploymentGroupId: null,
  managerId: null,
  enabled: false,
  createdAt: new Date("2026-04-29T17:55:39.547Z"),
  expiresAt: new Date("2026-08-02T13:34:20.382Z"),
  lastUsedAt: new Date("2024-02-27T10:48:41.580Z"),
  revokedAt: new Date("2024-12-29T17:42:58.373Z"),
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
  createdByUser: {
    id: "<id>",
    name: "<value>",
    email: "Gladys.OKon86@hotmail.com",
    image: "https://picsum.photos/seed/nT6Nfv/1927/2396",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the api key.                                                            | apikey_ye96yxs1tjnrrwulp8frh6w5t6e0                                                           |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `keyPrefix`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `type`                                                                                        | [models.APIKeyType](../models/apikeytype.md)                                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
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
| `createdByUser`                                                                               | [models.CreatedByUser](../models/createdbyuser.md)                                            | :heavy_check_mark:                                                                            | User information associated with the API key                                                  |                                                                                               |