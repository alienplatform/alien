# SyncReconcileRequestCurrentReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseOverridePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                    | [models.SyncReconcileRequestCurrentReleaseOverrideAw](../models/syncreconcilerequestcurrentreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                       | AWS permission configurations                                                                                            |
| `azure`                                                                                                                  | [models.SyncReconcileRequestCurrentReleaseOverrideAzure](../models/syncreconcilerequestcurrentreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                       | Azure permission configurations                                                                                          |
| `gcp`                                                                                                                    | [models.SyncReconcileRequestCurrentReleaseOverrideGcp](../models/syncreconcilerequestcurrentreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                       | GCP permission configurations                                                                                            |