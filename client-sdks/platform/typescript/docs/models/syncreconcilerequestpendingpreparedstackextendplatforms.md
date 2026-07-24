# SyncReconcileRequestPendingPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                            | [models.SyncReconcileRequestPendingPreparedStackExtendAw](../models/syncreconcilerequestpendingpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                               | AWS permission configurations                                                                                                    |
| `azure`                                                                                                                          | [models.SyncReconcileRequestPendingPreparedStackExtendAzure](../models/syncreconcilerequestpendingpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                               | Azure permission configurations                                                                                                  |
| `gcp`                                                                                                                            | [models.SyncReconcileRequestPendingPreparedStackExtendGcp](../models/syncreconcilerequestpendingpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                               | GCP permission configurations                                                                                                    |
