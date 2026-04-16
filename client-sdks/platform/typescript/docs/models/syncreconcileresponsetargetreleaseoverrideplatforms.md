# SyncReconcileResponseTargetReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                    | [models.SyncReconcileResponseTargetReleaseOverrideAw](../models/syncreconcileresponsetargetreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                       | AWS permission configurations                                                                                            |
| `azure`                                                                                                                  | [models.SyncReconcileResponseTargetReleaseOverrideAzure](../models/syncreconcileresponsetargetreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                       | Azure permission configurations                                                                                          |
| `gcp`                                                                                                                    | [models.SyncReconcileResponseTargetReleaseOverrideGcp](../models/syncreconcileresponsetargetreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                       | GCP permission configurations                                                                                            |