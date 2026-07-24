# SyncReconcileRequestPendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                | [models.SyncReconcileRequestPendingPreparedStackOverrideAw](../models/syncreconcilerequestpendingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                   | AWS permission configurations                                                                                                        |
| `azure`                                                                                                                              | [models.SyncReconcileRequestPendingPreparedStackOverrideAzure](../models/syncreconcilerequestpendingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                   | Azure permission configurations                                                                                                      |
| `gcp`                                                                                                                                | [models.SyncReconcileRequestPendingPreparedStackOverrideGcp](../models/syncreconcilerequestpendingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                   | GCP permission configurations                                                                                                        |
