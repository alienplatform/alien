# PendingSetupLinkResponse

## Example Usage

```typescript
import { PendingSetupLinkResponse } from "@alienplatform/platform-api/models";

let value: PendingSetupLinkResponse = {
  managerUrl: "https://merry-deer.org",
  token: "<value>",
  scope: "<value>",
  expiresAt: new Date("2024-06-19T17:27:46.524Z"),
  linkId: "apikey_ye96yxs1tjnrrwulp8frh6w5t6e0",
  pendingExpiresAt: new Date("2025-03-19T12:44:32.227Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `managerUrl`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Platform JWT for the manager's values API. Valid for 5 minutes.                               |                                                                                               |
| `scope`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Value scope on the manager, such as dep:dep_123.                                              |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `linkId`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | The link. It cannot create deployments until it is activated.                                 | apikey_ye96yxs1tjnrrwulp8frh6w5t6e0                                                           |
| `pendingExpiresAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the pending link is discarded and its values removed.                                    |                                                                                               |