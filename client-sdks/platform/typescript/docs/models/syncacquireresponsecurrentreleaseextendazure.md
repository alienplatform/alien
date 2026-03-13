# SyncAcquireResponseCurrentReleaseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncAcquireResponseCurrentReleaseExtendAzureBinding](../models/syncacquireresponsecurrentreleaseextendazurebinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncAcquireResponseCurrentReleaseExtendAzureGrant](../models/syncacquireresponsecurrentreleaseextendazuregrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |