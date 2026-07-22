# SyncReconcileResponsePendingPreparedStackProfile

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackProfile } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackProfile = {
  description:
    "catalyze consequently forenenst smuggle pomelo stealthily keenly",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `description`                                                                                                                              | *string*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Human-readable description of what this permission set allows                                                                              |
| `id`                                                                                                                                       | *string*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Unique identifier for the permission set (e.g., "storage/data-read")                                                                       |
| `platforms`                                                                                                                                | [models.SyncReconcileResponsePendingPreparedStackProfilePlatforms](../models/syncreconcileresponsependingpreparedstackprofileplatforms.md) | :heavy_check_mark:                                                                                                                         | Platform-specific permission configurations                                                                                                |
