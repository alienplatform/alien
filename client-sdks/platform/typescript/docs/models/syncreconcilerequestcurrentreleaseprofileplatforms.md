# SyncReconcileRequestCurrentReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncReconcileRequestCurrentReleaseProfileAw](../models/syncreconcilerequestcurrentreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncReconcileRequestCurrentReleaseProfileAzure](../models/syncreconcilerequestcurrentreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncReconcileRequestCurrentReleaseProfileGcp](../models/syncreconcilerequestcurrentreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |