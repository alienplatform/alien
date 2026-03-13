# SyncReconcileResponsePreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                    | [models.SyncReconcileResponsePreparedStackOverrideAw](../models/syncreconcileresponsepreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                       | AWS permission configurations                                                                                            |
| `azure`                                                                                                                  | [models.SyncReconcileResponsePreparedStackOverrideAzure](../models/syncreconcileresponsepreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                       | Azure permission configurations                                                                                          |
| `gcp`                                                                                                                    | [models.SyncReconcileResponsePreparedStackOverrideGcp](../models/syncreconcileresponsepreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                       | GCP permission configurations                                                                                            |