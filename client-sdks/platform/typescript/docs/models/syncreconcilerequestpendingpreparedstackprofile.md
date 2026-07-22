# SyncReconcileRequestPendingPreparedStackProfile

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackProfile } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackProfile = {
  description: "amidst unaccountably so brr sermon ugh violent",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                            | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Human-readable description of what this permission set allows                                                                            |
| `id`                                                                                                                                     | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Unique identifier for the permission set (e.g., "storage/data-read")                                                                     |
| `platforms`                                                                                                                              | [models.SyncReconcileRequestPendingPreparedStackProfilePlatforms](../models/syncreconcilerequestpendingpreparedstackprofileplatforms.md) | :heavy_check_mark:                                                                                                                       | Platform-specific permission configurations                                                                                              |
