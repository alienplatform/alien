# CreateAPIKeyRequest

Request schema for creating a new API key

## Example Usage

```typescript
import { CreateAPIKeyRequest } from "@aliendotdev/platform-api/models";

let value: CreateAPIKeyRequest = {
  description: "inside hence fast bad",
  scope: {
    type: "manager",
    managerId: "<id>",
    role: "manager.runtime",
  },
  expiresAt: new Date("2026-11-07T23:36:41.514Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `scope`                                                                                       | *models.Scope*                                                                                | :heavy_check_mark:                                                                            | Scope and role configuration for service accounts                                             |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Optional expiration date for the API key                                                      |