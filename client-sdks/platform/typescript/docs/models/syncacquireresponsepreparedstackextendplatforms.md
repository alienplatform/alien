# SyncAcquireResponsePreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackExtendPlatforms } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                            | [models.SyncAcquireResponsePreparedStackExtendAw](../models/syncacquireresponsepreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                               | AWS permission configurations                                                                                    |
| `azure`                                                                                                          | [models.SyncAcquireResponsePreparedStackExtendAzure](../models/syncacquireresponsepreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                               | Azure permission configurations                                                                                  |
| `gcp`                                                                                                            | [models.SyncAcquireResponsePreparedStackExtendGcp](../models/syncacquireresponsepreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                               | GCP permission configurations                                                                                    |