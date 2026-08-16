# ContainerRegistryManagerSnapshotCredential

## Example Usage

```typescript
import { ContainerRegistryManagerSnapshotCredential } from "@alienplatform/platform-api/models";

let value: ContainerRegistryManagerSnapshotCredential = {
  id: "crcred_oz1xjr82f37j17g4gtmyu",
  secretPrefix: "<value>",
  secretDigest: "<value>",
  scope: "pull",
  repositorySubset: [
    "<value 1>",
  ],
  expiresAt: new Date("2026-12-30T16:41:58.517Z"),
  revokedAt: new Date("2026-08-15T16:24:47.001Z"),
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        | Example                                                                                            |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `id`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | Unique identifier for the container registry credential.                                           | crcred_oz1xjr82f37j17g4gtmyu                                                                       |
| `secretPrefix`                                                                                     | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |
| `secretDigest`                                                                                     | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |
| `scope`                                                                                            | [models.ContainerRegistryManagerSnapshotScope](../models/containerregistrymanagersnapshotscope.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |
| `repositorySubset`                                                                                 | *string*[]                                                                                         | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |
| `expiresAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |
| `revokedAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |