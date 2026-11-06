# CreateAPIKeyResponse

Response containing the new API key and its metadata

## Example Usage

```typescript
import { CreateAPIKeyResponse } from "@alienplatform/platform-api/models";

let value: CreateAPIKeyResponse = {
  apiKey: "<value>",
  keyInfo: {
    id: "apikey_ye96yxs1tjnrrwulp8frh6w5t6e0",
    description: "vice leading schlep yahoo demob um",
    keyPrefix: "<value>",
    type: "manager",
    role: "<value>",
    additionalRoles: [
      "project.encryption",
    ],
    workspaceId: "<id>",
    projectId: "<id>",
    deploymentId: "<id>",
    deploymentGroupId: "<id>",
    managerId: "<id>",
    enabled: true,
    createdAt: new Date("2024-02-24T06:55:32.074Z"),
    expiresAt: new Date("2024-06-18T02:25:10.121Z"),
    lastUsedAt: new Date("2026-05-07T23:47:05.462Z"),
    revokedAt: new Date("2024-04-10T14:18:39.161Z"),
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
  },
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `apiKey`                                      | *string*                                      | :heavy_check_mark:                            | The generated API key value (only shown once) |
| `keyInfo`                                     | [models.KeyInfo](../models/keyinfo.md)        | :heavy_check_mark:                            | N/A                                           |