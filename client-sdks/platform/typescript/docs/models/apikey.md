# APIKey

API key information

## Example Usage

```typescript
import { APIKey } from "@alienplatform/platform-api/models";

let value: APIKey = {
  id: "apikey_ye96yxs1tjnrrwulp8frh",
  description: "hoarse forsaken slowly behind anaesthetise",
  keyPrefix: "<value>",
  type: "manager",
  role: "<value>",
  workspaceId: "<id>",
  projectId: "<id>",
  deploymentId: "<id>",
  deploymentGroupId: "<id>",
  managerId: "<id>",
  enabled: true,
  createdAt: new Date("2024-03-11T02:58:26.815Z"),
  expiresAt: new Date("2026-04-29T17:55:39.547Z"),
  lastUsedAt: new Date("2026-08-02T13:34:20.382Z"),
  revokedAt: new Date("2024-02-27T10:48:41.580Z"),
  createdByUser: {
    id: "<id>",
    email: "Domenick70@hotmail.com",
    image: "https://picsum.photos/seed/uBkY5PS5f/1154/1987",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the api key.                                                            | apikey_ye96yxs1tjnrrwulp8frh                                                                  |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `keyPrefix`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `type`                                                                                        | [models.APIKeyType](../models/apikeytype.md)                                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `role`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
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
| `createdByUser`                                                                               | [models.CreatedByUser](../models/createdbyuser.md)                                            | :heavy_check_mark:                                                                            | User information associated with the API key                                                  |                                                                                               |