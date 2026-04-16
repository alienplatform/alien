# SyncReconcileRequestTargetReleaseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseOverridePlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncReconcileRequestTargetReleaseOverrideAw](../models/syncreconcilerequesttargetreleaseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncReconcileRequestTargetReleaseOverrideAzure](../models/syncreconcilerequesttargetreleaseoverrideazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncReconcileRequestTargetReleaseOverrideGcp](../models/syncreconcilerequesttargetreleaseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |