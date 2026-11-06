# PendingChange

## Example Usage

```typescript
import { PendingChange } from "@alienplatform/platform-api/models";

let value: PendingChange = {
  id: "pch_gz7jg0o8yf5m0b0h8uanp2tajfon",
  projectId: "<id>",
  status: "pending",
  summary: "<value>",
  basedOnVersion: 919599,
  body: {
    "key": "<value>",
    "key1": "<value>",
  },
  requestedBy: "<value>",
  createdAt: new Date("2024-11-29T11:49:01.550Z"),
  resolvedAt: new Date("2026-04-11T04:20:18.600Z"),
  signerListVersion: 510270,
};
```

## Fields

| Field                                                                                           | Type                                                                                            | Required                                                                                        | Description                                                                                     | Example                                                                                         |
| ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `id`                                                                                            | *string*                                                                                        | :heavy_check_mark:                                                                              | Also the request id the pairing code covers                                                     | pch_gz7jg0o8yf5m0b0h8uanp2tajfon                                                                |
| `projectId`                                                                                     | *string*                                                                                        | :heavy_check_mark:                                                                              | N/A                                                                                             |                                                                                                 |
| `status`                                                                                        | [models.PendingChangeStatus](../models/pendingchangestatus.md)                                  | :heavy_check_mark:                                                                              | N/A                                                                                             |                                                                                                 |
| `summary`                                                                                       | *string*                                                                                        | :heavy_check_mark:                                                                              | One line for lists, such as `bob-thinkpad asks to: release`. Display only.                      |                                                                                                 |
| `basedOnVersion`                                                                                | *number*                                                                                        | :heavy_check_mark:                                                                              | List version the body was prepared from                                                         |                                                                                                 |
| `body`                                                                                          | Record<string, *any*>                                                                           | :heavy_check_mark:                                                                              | Body of the next signer list: previous, adminThreshold, signers, deny, recovered                |                                                                                                 |
| `requestedBy`                                                                                   | *string*                                                                                        | :heavy_check_mark:                                                                              | Email or API key of whoever filed it; null for the refresh the platform prepares. Display only. |                                                                                                 |
| `createdAt`                                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)   | :heavy_check_mark:                                                                              | N/A                                                                                             |                                                                                                 |
| `resolvedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)   | :heavy_check_mark:                                                                              | N/A                                                                                             |                                                                                                 |
| `signerListVersion`                                                                             | *number*                                                                                        | :heavy_check_mark:                                                                              | The list version that carries the change, once approved                                         |                                                                                                 |