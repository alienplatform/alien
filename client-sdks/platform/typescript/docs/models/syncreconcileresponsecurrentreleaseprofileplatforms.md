# SyncReconcileResponseCurrentReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                    | [models.SyncReconcileResponseCurrentReleaseProfileAw](../models/syncreconcileresponsecurrentreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                       | AWS permission configurations                                                                                            |
| `azure`                                                                                                                  | [models.SyncReconcileResponseCurrentReleaseProfileAzure](../models/syncreconcileresponsecurrentreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                       | Azure permission configurations                                                                                          |
| `gcp`                                                                                                                    | [models.SyncReconcileResponseCurrentReleaseProfileGcp](../models/syncreconcileresponsecurrentreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                       | GCP permission configurations                                                                                            |