# SyncReconcileResponsePendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                  | [models.SyncReconcileResponsePendingPreparedStackOverrideAw](../models/syncreconcileresponsependingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                     | AWS permission configurations                                                                                                          |
| `azure`                                                                                                                                | [models.SyncReconcileResponsePendingPreparedStackOverrideAzure](../models/syncreconcileresponsependingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                     | Azure permission configurations                                                                                                        |
| `gcp`                                                                                                                                  | [models.SyncReconcileResponsePendingPreparedStackOverrideGcp](../models/syncreconcileresponsependingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                     | GCP permission configurations                                                                                                          |
