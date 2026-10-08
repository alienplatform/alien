# SyncListResponsePendingPreparedStackExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                            | [models.SyncListResponsePendingPreparedStackExtendAzureBinding](../models/synclistresponsependingpreparedstackextendazurebinding.md) | :heavy_check_mark:                                                                                                                   | Generic binding configuration for permissions                                                                                        |
| `description`                                                                                                                        | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | Short admin-facing description of why this entry exists.                                                                             |
| `grant`                                                                                                                              | [models.SyncListResponsePendingPreparedStackExtendAzureGrant](../models/synclistresponsependingpreparedstackextendazuregrant.md)     | :heavy_check_mark:                                                                                                                   | Grant permissions for a specific cloud platform                                                                                      |
| `label`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | Stable admin-facing label for this permission entry.                                                                                 |
