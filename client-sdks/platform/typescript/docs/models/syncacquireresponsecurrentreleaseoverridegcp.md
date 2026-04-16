# SyncAcquireResponseCurrentReleaseOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseOverrideGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncAcquireResponseCurrentReleaseOverrideGcpBinding](../models/syncacquireresponsecurrentreleaseoverridegcpbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncAcquireResponseCurrentReleaseOverrideGcpGrant](../models/syncacquireresponsecurrentreleaseoverridegcpgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |