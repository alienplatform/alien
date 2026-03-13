# SyncReconcileRequestPreparedStackExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackExtend } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestPreparedStackExtend = {
  description:
    "underpants psst because how around majestically profuse transplant brilliant miscalculate",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `description`                                                                                                            | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Human-readable description of what this permission set allows                                                            |
| `id`                                                                                                                     | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Unique identifier for the permission set (e.g., "storage/data-read")                                                     |
| `platforms`                                                                                                              | [models.SyncReconcileRequestPreparedStackExtendPlatforms](../models/syncreconcilerequestpreparedstackextendplatforms.md) | :heavy_check_mark:                                                                                                       | Platform-specific permission configurations                                                                              |