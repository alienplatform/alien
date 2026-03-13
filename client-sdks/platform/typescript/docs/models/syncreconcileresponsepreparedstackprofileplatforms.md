# SyncReconcileResponsePreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncReconcileResponsePreparedStackProfileAw](../models/syncreconcileresponsepreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncReconcileResponsePreparedStackProfileAzure](../models/syncreconcileresponsepreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncReconcileResponsePreparedStackProfileGcp](../models/syncreconcileresponsepreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |