# SyncReconcileResponsePreparedStackProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackProfileAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileResponsePreparedStackProfileAwBinding](../models/syncreconcileresponsepreparedstackprofileawbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileResponsePreparedStackProfileAwGrant](../models/syncreconcileresponsepreparedstackprofileawgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |