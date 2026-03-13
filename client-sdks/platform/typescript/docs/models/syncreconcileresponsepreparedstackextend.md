# SyncReconcileResponsePreparedStackExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackExtend } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePreparedStackExtend = {
  description:
    "unless king healthily sternly till indeed calmly violently electronics",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                              | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Human-readable description of what this permission set allows                                                              |
| `id`                                                                                                                       | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Unique identifier for the permission set (e.g., "storage/data-read")                                                       |
| `platforms`                                                                                                                | [models.SyncReconcileResponsePreparedStackExtendPlatforms](../models/syncreconcileresponsepreparedstackextendplatforms.md) | :heavy_check_mark:                                                                                                         | Platform-specific permission configurations                                                                                |