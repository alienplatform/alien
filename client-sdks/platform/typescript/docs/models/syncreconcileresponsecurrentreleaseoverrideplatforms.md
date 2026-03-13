# SyncReconcileResponseCurrentReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseOverridePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                      | [models.SyncReconcileResponseCurrentReleaseOverrideAw](../models/syncreconcileresponsecurrentreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                         | AWS permission configurations                                                                                              |
| `azure`                                                                                                                    | [models.SyncReconcileResponseCurrentReleaseOverrideAzure](../models/syncreconcileresponsecurrentreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                         | Azure permission configurations                                                                                            |
| `gcp`                                                                                                                      | [models.SyncReconcileResponseCurrentReleaseOverrideGcp](../models/syncreconcileresponsecurrentreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                         | GCP permission configurations                                                                                              |