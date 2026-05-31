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
      },
      createdByUser: {
        id: "<id>",
        email: "Rhianna90@hotmail.com",
        image: "https://picsum.photos/seed/UEmR2Mt/3119/3794",
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