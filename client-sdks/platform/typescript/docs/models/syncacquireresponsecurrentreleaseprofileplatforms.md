# SyncAcquireResponseCurrentReleaseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseProfilePlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncAcquireResponseCurrentReleaseProfileAw](../models/syncacquireresponsecurrentreleaseprofileaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncAcquireResponseCurrentReleaseProfileAzure](../models/syncacquireresponsecurrentreleaseprofileazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncAcquireResponseCurrentReleaseProfileGcp](../models/syncacquireresponsecurrentreleaseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |