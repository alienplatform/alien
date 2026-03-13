# SyncReconcileRequestPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                              | [models.SyncReconcileRequestPreparedStackExtendAw](../models/syncreconcilerequestpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                 | AWS permission configurations                                                                                      |
| `azure`                                                                                                            | [models.SyncReconcileRequestPreparedStackExtendAzure](../models/syncreconcilerequestpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                 | Azure permission configurations                                                                                    |
| `gcp`                                                                                                              | [models.SyncReconcileRequestPreparedStackExtendGcp](../models/syncreconcilerequestpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                 | GCP permission configurations                                                                                      |