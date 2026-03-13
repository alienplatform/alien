# SyncReconcileRequestTargetReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseExtendPlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                              | [models.SyncReconcileRequestTargetReleaseExtendAw](../models/syncreconcilerequesttargetreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                 | AWS permission configurations                                                                                      |
| `azure`                                                                                                            | [models.SyncReconcileRequestTargetReleaseExtendAzure](../models/syncreconcilerequesttargetreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                 | Azure permission configurations                                                                                    |
| `gcp`                                                                                                              | [models.SyncReconcileRequestTargetReleaseExtendGcp](../models/syncreconcilerequesttargetreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                 | GCP permission configurations                                                                                      |