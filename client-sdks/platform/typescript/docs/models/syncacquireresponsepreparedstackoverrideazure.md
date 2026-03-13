# SyncAcquireResponsePreparedStackOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackOverrideAzure } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncAcquireResponsePreparedStackOverrideAzureBinding](../models/syncacquireresponsepreparedstackoverrideazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncAcquireResponsePreparedStackOverrideAzureGrant](../models/syncacquireresponsepreparedstackoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |