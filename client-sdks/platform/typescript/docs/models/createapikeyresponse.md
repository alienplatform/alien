# CreateAPIKeyResponse

Response containing the new API key and its metadata

## Example Usage

```typescript
import { CreateAPIKeyResponse } from "@aliendotdev/platform-api/models";

let value: CreateAPIKeyResponse = {
  apiKey: "<value>",
  keyInfo: {
    id: "apikey_ye96yxs1tjnrrwulp8frh",
    description: "vice leading schlep yahoo demob um",
    keyPrefix: "<value>",
    type: "manager",
    role: "<value>",
    workspaceId: "<id>",
    projectId: "<id>",
    deploymentId: null,
    deploymentGroupId: "<id>",
    managerId: "<id>",
    enabled: false,
    createdAt: new Date("2025-12-31T21:17:56.658Z"),
    expiresAt: new Date("2024-02-24T06:55:32.074Z"),
    lastUsedAt: new Date("2024-06-18T02:25:10.121Z"),
    revokedAt: new Date("2026-05-07T23:47:05.462Z"),
  },
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `apiKey`                                      | *string*                                      | :heavy_check_mark:                            | The generated API key value (only shown once) |
| `keyInfo`                                     | [models.KeyInfo](../models/keyinfo.md)        | :heavy_check_mark:                            | N/A                                           |