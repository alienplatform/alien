# ListAPIKeysResponse

Paginated response

## Example Usage

```typescript
import { ListAPIKeysResponse } from "@alienplatform/platform-api/models/operations";

let value: ListAPIKeysResponse = {
  items: [
    {
      id: "apikey_ye96yxs1tjnrrwulp8frh6w5t6e0",
      description: "that editor whenever inwardly without circulate disapprove",
      keyPrefix: "<value>",
      type: "deployment-group",
      role: "<value>",
      additionalRoles: [],
      workspaceId: "<id>",
      projectId: "<id>",
      deploymentId: "<id>",
      deploymentGroupId: "<id>",
      managerId: "<id>",
      enabled: false,
      createdAt: new Date("2024-01-28T14:26:10.721Z"),
      expiresAt: new Date("2026-05-04T15:57:19.301Z"),
      lastUsedAt: null,
      revokedAt: new Date("2026-06-24T05:33:12.679Z"),
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
    },
  ],
  nextCursor: null,
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `items`                                     | [models.APIKey](../../models/apikey.md)[]   | :heavy_check_mark:                          | Items in this page                          |
| `nextCursor`                                | *string*                                    | :heavy_check_mark:                          | Cursor for the next page, null if last page |