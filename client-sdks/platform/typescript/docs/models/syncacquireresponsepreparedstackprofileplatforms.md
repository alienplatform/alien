# SyncAcquireResponsePreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                              | [models.SyncAcquireResponsePreparedStackProfileAw](../models/syncacquireresponsepreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                 | AWS permission configurations                                                                                      |
| `azure`                                                                                                            | [models.SyncAcquireResponsePreparedStackProfileAzure](../models/syncacquireresponsepreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                 | Azure permission configurations                                                                                    |
| `gcp`                                                                                                              | [models.SyncAcquireResponsePreparedStackProfileGcp](../models/syncacquireresponsepreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                 | GCP permission configurations                                                                                      |