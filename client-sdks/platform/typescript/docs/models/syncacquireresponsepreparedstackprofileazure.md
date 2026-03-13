# SyncAcquireResponsePreparedStackProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackProfileAzure } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncAcquireResponsePreparedStackProfileAzureBinding](../models/syncacquireresponsepreparedstackprofileazurebinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncAcquireResponsePreparedStackProfileAzureGrant](../models/syncacquireresponsepreparedstackprofileazuregrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |