# PendingChangeResponse

## Example Usage

```typescript
import { PendingChangeResponse } from "@alienplatform/manager-api/models";

let value: PendingChangeResponse = {
  basedOnVersion: 256426,
  body: {},
  createdAt: "1727134660378",
  id: "<id>",
  projectId: "<id>",
  status: "approved",
  summary: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `basedOnVersion`                                                           | *number*                                                                   | :heavy_check_mark:                                                         | Signer list version the body was prepared from.                            |
| `body`                                                                     | [models.PendingChangeResponseBody](../models/pendingchangeresponsebody.md) | :heavy_check_mark:                                                         | The next list's body.                                                      |
| `createdAt`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `id`                                                                       | *string*                                                                   | :heavy_check_mark:                                                         | Also the request id the pairing code covers.                               |
| `projectId`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `requestedBy`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | Who filed it, display only.                                                |
| `resolvedAt`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `signerListVersion`                                                        | *number*                                                                   | :heavy_minus_sign:                                                         | Set when approved.                                                         |
| `status`                                                                   | [models.PendingChangeStatus](../models/pendingchangestatus.md)             | :heavy_check_mark:                                                         | Where a pending change stands.                                             |
| `summary`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | Display only.                                                              |
