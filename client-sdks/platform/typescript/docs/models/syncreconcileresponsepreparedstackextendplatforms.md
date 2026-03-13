# SyncReconcileResponsePreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackExtendPlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncReconcileResponsePreparedStackExtendAw](../models/syncreconcileresponsepreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncReconcileResponsePreparedStackExtendAzure](../models/syncreconcileresponsepreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncReconcileResponsePreparedStackExtendGcp](../models/syncreconcileresponsepreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |