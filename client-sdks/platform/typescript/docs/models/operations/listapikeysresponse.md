# ListAPIKeysResponse

Paginated response

## Example Usage

```typescript
import { ListAPIKeysResponse } from "@alienplatform/platform-api/models/operations";

let value: ListAPIKeysResponse = {
  items: [
    {
      id: "apikey_ye96yxs1tjnrrwulp8frh",
      description: "that editor whenever inwardly without circulate disapprove",
      keyPrefix: "<value>",
      type: "deployment-group",
      role: "<value>",
      workspaceId: "<id>",
      projectId: "<id>",
      deploymentId: "<id>",
      deploymentGroupId: "<id>",
      managerId: "<id>",
      enabled: true,
      createdAt: new Date("2026-04-22T16:09:38.190Z"),
      expiresAt: null,
      lastUsedAt: new Date("2026-05-04T15:57:19.301Z"),
      revokedAt: null,
      deploymentSetupConfig: {
        metadata: {
          "key": "<value>",
        },
        policy: {
          allowedPlatforms: [],
          allowedSetupMethods: [
            "google-oauth",
          ],
        },
        environmentVariables: [],
        items: [
          {
            item: "models",
            source: {
              type: "built-in",
              definitionId: "customer-key",
              version: "<value>",
              sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
            },
            required: false,
          },
        ],
      },
      createdByUser: {
        id: "<id>",
        email: "Sydnee_Fahey-Satterfield@gmail.com",
        image: "https://picsum.photos/seed/MtBCj/2624/1426",
      },
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `items`                                     | [models.APIKey](../../models/apikey.md)[]   | :heavy_check_mark:                          | Items in this page                          |
| `nextCursor`                                | *string*                                    | :heavy_check_mark:                          | Cursor for the next page, null if last page |