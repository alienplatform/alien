# SyncReconcileRequestPendingPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                              | [models.SyncReconcileRequestPendingPreparedStackProfileAw](../models/syncreconcilerequestpendingpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                                 | AWS permission configurations                                                                                                      |
| `azure`                                                                                                                            | [models.SyncReconcileRequestPendingPreparedStackProfileAzure](../models/syncreconcilerequestpendingpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                                 | Azure permission configurations                                                                                                    |
| `gcp`                                                                                                                              | [models.SyncReconcileRequestPendingPreparedStackProfileGcp](../models/syncreconcilerequestpendingpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                                 | GCP permission configurations                                                                                                      |
