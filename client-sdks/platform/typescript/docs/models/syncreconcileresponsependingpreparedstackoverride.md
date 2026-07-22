# SyncReconcileResponsePendingPreparedStackOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackOverride } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackOverride = {
  description: "fatally the equal whimsical for coal bare rowdy flint",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                                | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | Human-readable description of what this permission set allows                                                                                |
| `id`                                                                                                                                         | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | Unique identifier for the permission set (e.g., "storage/data-read")                                                                         |
| `platforms`                                                                                                                                  | [models.SyncReconcileResponsePendingPreparedStackOverridePlatforms](../models/syncreconcileresponsependingpreparedstackoverrideplatforms.md) | :heavy_check_mark:                                                                                                                           | Platform-specific permission configurations                                                                                                  |
