# SyncReconcileResponseTargetReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTargetReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncReconcileResponseTargetReleaseProfileAw](../models/syncreconcileresponsetargetreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncReconcileResponseTargetReleaseProfileAzure](../models/syncreconcileresponsetargetreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncReconcileResponseTargetReleaseProfileGcp](../models/syncreconcileresponsetargetreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |