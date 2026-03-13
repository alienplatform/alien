# SyncAcquireResponseTargetReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseOverridePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncAcquireResponseTargetReleaseOverrideAw](../models/syncacquireresponsetargetreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncAcquireResponseTargetReleaseOverrideAzure](../models/syncacquireresponsetargetreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncAcquireResponseTargetReleaseOverrideGcp](../models/syncacquireresponsetargetreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |