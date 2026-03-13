# SyncAcquireResponseCurrentReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseOverridePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncAcquireResponseCurrentReleaseOverrideAw](../models/syncacquireresponsecurrentreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncAcquireResponseCurrentReleaseOverrideAzure](../models/syncacquireresponsecurrentreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncAcquireResponseCurrentReleaseOverrideGcp](../models/syncacquireresponsecurrentreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |