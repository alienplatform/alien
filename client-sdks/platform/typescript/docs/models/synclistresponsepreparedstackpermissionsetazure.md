# SyncListResponsePreparedStackPermissionSetAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncListResponsePreparedStackPermissionSetAzure } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackPermissionSetAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                            | [models.SyncListResponsePreparedStackPermissionSetAzureBinding](../models/synclistresponsepreparedstackpermissionsetazurebinding.md) | :heavy_check_mark:                                                                                                                   | Generic binding configuration for permissions                                                                                        |
| `description`                                                                                                                        | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | Short admin-facing description of why this entry exists.                                                                             |
| `grant`                                                                                                                              | [models.SyncListResponsePreparedStackPermissionSetAzureGrant](../models/synclistresponsepreparedstackpermissionsetazuregrant.md)     | :heavy_check_mark:                                                                                                                   | Grant permissions for a specific cloud platform                                                                                      |
| `label`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | Stable admin-facing label for this permission entry.                                                                                 |