# SyncReconcileRequestPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackProfilePlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestPreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncReconcileRequestPreparedStackProfileAw](../models/syncreconcilerequestpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncReconcileRequestPreparedStackProfileAzure](../models/syncreconcilerequestpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncReconcileRequestPreparedStackProfileGcp](../models/syncreconcilerequestpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |