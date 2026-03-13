# SyncAcquireResponsePreparedStackOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackOverrideGcp } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncAcquireResponsePreparedStackOverrideGcpBinding](../models/syncacquireresponsepreparedstackoverridegcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncAcquireResponsePreparedStackOverrideGcpGrant](../models/syncacquireresponsepreparedstackoverridegcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |