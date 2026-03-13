# SyncReconcileRequestTargetReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncReconcileRequestTargetReleaseProfileAw](../models/syncreconcilerequesttargetreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncReconcileRequestTargetReleaseProfileAzure](../models/syncreconcilerequesttargetreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncReconcileRequestTargetReleaseProfileGcp](../models/syncreconcilerequesttargetreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |