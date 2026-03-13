# SyncReconcileRequestPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackOverridePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncReconcileRequestPreparedStackOverrideAw](../models/syncreconcilerequestpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncReconcileRequestPreparedStackOverrideAzure](../models/syncreconcilerequestpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncReconcileRequestPreparedStackOverrideGcp](../models/syncreconcilerequestpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |