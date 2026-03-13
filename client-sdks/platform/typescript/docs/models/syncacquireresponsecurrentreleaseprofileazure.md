# SyncAcquireResponseCurrentReleaseProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseProfileAzure } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncAcquireResponseCurrentReleaseProfileAzureBinding](../models/syncacquireresponsecurrentreleaseprofileazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncAcquireResponseCurrentReleaseProfileAzureGrant](../models/syncacquireresponsecurrentreleaseprofileazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |