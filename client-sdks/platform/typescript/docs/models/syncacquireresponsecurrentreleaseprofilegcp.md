# SyncAcquireResponseCurrentReleaseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseProfileGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncAcquireResponseCurrentReleaseProfileGcpBinding](../models/syncacquireresponsecurrentreleaseprofilegcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncAcquireResponseCurrentReleaseProfileGcpGrant](../models/syncacquireresponsecurrentreleaseprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |