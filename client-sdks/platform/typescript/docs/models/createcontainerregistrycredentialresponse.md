# CreateContainerRegistryCredentialResponse

## Example Usage

```typescript
import { CreateContainerRegistryCredentialResponse } from "@alienplatform/platform-api/models";

let value: CreateContainerRegistryCredentialResponse = {
  credential: {
    id: "crcred_oz1xjr82f37j17g4gtmyu",
    label: "<value>",
    scope: "pull",
    repositorySubset: [],
    expiresAt: new Date("2025-12-20T20:26:57.148Z"),
    lastUsedAt: new Date("2026-02-06T06:37:07.935Z"),
    revokedAt: new Date("2024-05-07T15:36:30.798Z"),
    createdAt: new Date("2024-08-07T10:25:48.379Z"),
  },
  username: "alien",
  password: "m2ADUHYmHdcnzc5",
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `credential`                                                                                                                   | [models.CreateContainerRegistryCredentialResponseCredential](../models/createcontainerregistrycredentialresponsecredential.md) | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `username`                                                                                                                     | [models.UsernameEnum](../models/usernameenum.md)                                                                               | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `password`                                                                                                                     | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |