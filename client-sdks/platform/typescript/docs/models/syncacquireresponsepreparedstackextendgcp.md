# SyncAcquireResponsePreparedStackExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackExtendGcp } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncAcquireResponsePreparedStackExtendGcpBinding](../models/syncacquireresponsepreparedstackextendgcpbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncAcquireResponsePreparedStackExtendGcpGrant](../models/syncacquireresponsepreparedstackextendgcpgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |